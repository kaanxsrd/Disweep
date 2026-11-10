use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use ts_rs::TS;

use crate::Snowflake;
use crate::engine::{Account, DayCount, FailureRecord, JobSnapshot, JobSpec, Rule, Stats};
use crate::package::PackageIndex;

const MAX_FAILURES_PER_JOB: i64 = 5_000;
const LATEST_SCHEMA: i32 = 1;

#[derive(Clone, Debug, thiserror::Error)]
#[error("Couldn't read or update Disweep's local data: {0}")]
pub struct StoreError(String);

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        Self(error.to_string())
    }
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Clone, Debug)]
pub struct JobRecord {
    pub spec: JobSpec,
    pub snapshot: JobSnapshot,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SettingEntry {
    pub key: String,
    pub value: String,
}

#[derive(Clone)]
pub struct Store {
    connection: Arc<Mutex<Connection>>,
}

impl Store {
    pub fn open(path: &Path) -> StoreResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| StoreError(error.to_string()))?;
        }
        Self::prepare(Connection::open(path)?)
    }

    pub fn in_memory() -> StoreResult<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(connection: Connection) -> StoreResult<Self> {
        connection.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;",
        )?;
        migrate(&connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    async fn call<T, F>(&self, operation: F) -> StoreResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> StoreResult<T> + Send + 'static,
    {
        let connection = self.connection.clone();
        tokio::task::spawn_blocking(move || operation(&connection.lock()))
            .await
            .map_err(|error| StoreError(error.to_string()))?
    }

    pub async fn save_account(&self, account: Account) -> StoreResult<()> {
        self.call(move |db| {
            db.execute(
                "INSERT INTO accounts (id, data, added_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET data = excluded.data",
                params![
                    account.id.to_string(),
                    serde_json::to_string(&account)?,
                    crate::now_ms()
                ],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn accounts(&self) -> StoreResult<Vec<Account>> {
        self.call(|db| select_json(db, "SELECT data FROM accounts ORDER BY added_at", params![]))
            .await
    }

    pub async fn remove_account(&self, id: Snowflake) -> StoreResult<()> {
        let id = id.to_string();
        self.call(move |db| {
            db.execute(
                "DELETE FROM failures WHERE job_id IN (SELECT id FROM jobs WHERE account_id = ?1)",
                params![id],
            )?;
            db.execute("DELETE FROM jobs WHERE account_id = ?1", params![id])?;
            db.execute("DELETE FROM rules WHERE account_id = ?1", params![id])?;
            db.execute("DELETE FROM stats_daily WHERE account_id = ?1", params![id])?;
            db.execute("DELETE FROM packages WHERE account_id = ?1", params![id])?;
            db.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
            Ok(())
        })
        .await
    }

    pub async fn save_job(&self, record: JobRecord) -> StoreResult<()> {
        self.call(move |db| {
            db.execute(
                "INSERT INTO jobs (id, account_id, spec, snapshot, state, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET spec = excluded.spec, snapshot = excluded.snapshot, state = excluded.state",
                params![
                    record.snapshot.id,
                    record.spec.account_id.to_string(),
                    serde_json::to_string(&record.spec)?,
                    serde_json::to_string(&record.snapshot)?,
                    serde_json::to_string(&record.snapshot.state)?,
                    record.snapshot.created_at,
                ],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn job(&self, id: String) -> StoreResult<Option<JobRecord>> {
        self.call(move |db| {
            let row = db
                .query_row(
                    "SELECT spec, snapshot FROM jobs WHERE id = ?1",
                    params![id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?;
            row.map(|(spec, snapshot)| {
                Ok(JobRecord {
                    spec: serde_json::from_str(&spec)?,
                    snapshot: serde_json::from_str(&snapshot)?,
                })
            })
            .transpose()
        })
        .await
    }

    pub async fn recent_jobs(&self, limit: u32) -> StoreResult<Vec<JobSnapshot>> {
        self.call(move |db| {
            select_json(
                db,
                "SELECT snapshot FROM jobs ORDER BY created_at DESC LIMIT ?1",
                params![limit],
            )
        })
        .await
    }

    pub async fn unfinished_jobs(&self) -> StoreResult<Vec<JobRecord>> {
        self.call(|db| {
            let mut statement = db.prepare(
                "SELECT spec, snapshot FROM jobs WHERE state IN ('\"queued\"', '\"running\"', '\"paused\"') ORDER BY created_at",
            )?;
            let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
            let mut records = Vec::new();
            for row in rows {
                let (spec, snapshot) = row?;
                records.push(JobRecord { spec: serde_json::from_str(&spec)?, snapshot: serde_json::from_str(&snapshot)? });
            }
            Ok(records)
        })
        .await
    }

    pub async fn delete_job(&self, id: String) -> StoreResult<()> {
        self.call(move |db| {
            db.execute("DELETE FROM failures WHERE job_id = ?1", params![id])?;
            db.execute("DELETE FROM jobs WHERE id = ?1", params![id])?;
            Ok(())
        })
        .await
    }

    pub async fn add_failure(&self, job_id: String, failure: FailureRecord) -> StoreResult<()> {
        self.call(move |db| {
            let count: i64 = db.query_row(
                "SELECT COUNT(*) FROM failures WHERE job_id = ?1",
                params![job_id],
                |row| row.get(0),
            )?;
            if count < MAX_FAILURES_PER_JOB {
                db.execute(
                    "INSERT INTO failures (job_id, data) VALUES (?1, ?2)",
                    params![job_id, serde_json::to_string(&failure)?],
                )?;
            }
            Ok(())
        })
        .await
    }

    pub async fn failures(&self, job_id: String) -> StoreResult<Vec<FailureRecord>> {
        self.call(move |db| {
            select_json(
                db,
                "SELECT data FROM failures WHERE job_id = ?1 ORDER BY rowid",
                params![job_id],
            )
        })
        .await
    }

    pub async fn add_deleted(&self, account: Snowflake, count: u64) -> StoreResult<()> {
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let account = account.to_string();
        self.call(move |db| {
            db.execute(
                "INSERT INTO stats_daily (account_id, day, deleted) VALUES (?1, ?2, ?3)
                 ON CONFLICT(account_id, day) DO UPDATE SET deleted = deleted + excluded.deleted",
                params![account, day, count as i64],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn add_deleted_on(
        &self,
        account: Snowflake,
        day: String,
        count: u64,
    ) -> StoreResult<()> {
        let account = account.to_string();
        self.call(move |db| {
            db.execute(
                "INSERT INTO stats_daily (account_id, day, deleted) VALUES (?1, ?2, ?3)
                 ON CONFLICT(account_id, day) DO UPDATE SET deleted = deleted + excluded.deleted",
                params![account, day, count as i64],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn stats(&self, account: Snowflake) -> StoreResult<Stats> {
        let account = account.to_string();
        self.call(move |db| {
            let total_deleted: i64 = db.query_row(
                "SELECT COALESCE(SUM(deleted), 0) FROM stats_daily WHERE account_id = ?1",
                params![account],
                |row| row.get(0),
            )?;
            let jobs_finished: i64 = db.query_row(
                "SELECT COUNT(*) FROM jobs WHERE account_id = ?1 AND state = '\"completed\"'",
                params![account],
                |row| row.get(0),
            )?;
            let mut statement = db.prepare(
                "SELECT day, deleted FROM stats_daily WHERE account_id = ?1 ORDER BY day DESC LIMIT 30",
            )?;
            let mut days = statement
                .query_map(params![account], |row| {
                    Ok(DayCount { day: row.get(0)?, deleted: row.get::<_, i64>(1)? as u64 })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            days.reverse();
            Ok(Stats { total_deleted: total_deleted as u64, jobs_finished: jobs_finished as u64, days })
        })
        .await
    }

    pub async fn save_rule(&self, rule: Rule) -> StoreResult<()> {
        self.call(move |db| {
            db.execute(
                "INSERT INTO rules (id, account_id, data) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET data = excluded.data",
                params![
                    rule.id,
                    rule.account_id.to_string(),
                    serde_json::to_string(&rule)?
                ],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn rules(&self) -> StoreResult<Vec<Rule>> {
        self.call(|db| select_json(db, "SELECT data FROM rules ORDER BY rowid", params![]))
            .await
    }

    pub async fn delete_rule(&self, id: String) -> StoreResult<()> {
        self.call(move |db| {
            db.execute("DELETE FROM rules WHERE id = ?1", params![id])?;
            Ok(())
        })
        .await
    }

    pub async fn save_package(&self, account: Snowflake, index: PackageIndex) -> StoreResult<()> {
        self.call(move |db| {
            db.execute(
                "INSERT INTO packages (account_id, data) VALUES (?1, ?2)
                 ON CONFLICT(account_id) DO UPDATE SET data = excluded.data",
                params![account.to_string(), serde_json::to_string(&index)?],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn package(&self, account: Snowflake) -> StoreResult<Option<PackageIndex>> {
        self.call(move |db| {
            let data: Option<String> = db
                .query_row(
                    "SELECT data FROM packages WHERE account_id = ?1",
                    params![account.to_string()],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(data.map(|data| serde_json::from_str(&data)).transpose()?)
        })
        .await
    }

    pub async fn remove_package(&self, account: Snowflake) -> StoreResult<()> {
        self.call(move |db| {
            db.execute(
                "DELETE FROM packages WHERE account_id = ?1",
                params![account.to_string()],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn setting(&self, key: String) -> StoreResult<Option<String>> {
        self.call(move |db| {
            Ok(db
                .query_row(
                    "SELECT value FROM settings WHERE key = ?1",
                    params![key],
                    |row| row.get(0),
                )
                .optional()?)
        })
        .await
    }

    pub async fn set_setting(&self, key: String, value: String) -> StoreResult<()> {
        self.call(move |db| {
            db.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn settings(&self) -> StoreResult<Vec<SettingEntry>> {
        self.call(|db| {
            let mut statement = db.prepare("SELECT key, value FROM settings ORDER BY key")?;
            let entries = statement
                .query_map([], |row| {
                    Ok(SettingEntry {
                        key: row.get(0)?,
                        value: row.get(1)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(entries)
        })
        .await
    }

    pub async fn wipe(&self) -> StoreResult<()> {
        self.call(|db| {
            db.execute_batch(
                "DELETE FROM failures; DELETE FROM jobs; DELETE FROM rules; DELETE FROM stats_daily;
                 DELETE FROM packages; DELETE FROM accounts; DELETE FROM settings;",
            )?;
            Ok(())
        })
        .await
    }
}

fn select_json<T: DeserializeOwned>(
    db: &Connection,
    sql: &str,
    parameters: impl rusqlite::Params,
) -> StoreResult<Vec<T>> {
    let mut statement = db.prepare(sql)?;
    let rows = statement.query_map(parameters, |row| row.get::<_, String>(0))?;
    let mut items = Vec::new();
    for row in rows {
        items.push(serde_json::from_str(&row?)?);
    }
    Ok(items)
}

fn migrate(db: &Connection) -> StoreResult<()> {
    let version: i32 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version < 1 {
        db.execute_batch(
            "BEGIN;
             CREATE TABLE accounts (id TEXT PRIMARY KEY, data TEXT NOT NULL, added_at INTEGER NOT NULL);
             CREATE TABLE jobs (
               id TEXT PRIMARY KEY, account_id TEXT NOT NULL, spec TEXT NOT NULL,
               snapshot TEXT NOT NULL, state TEXT NOT NULL, created_at INTEGER NOT NULL
             );
             CREATE INDEX jobs_by_time ON jobs (created_at DESC);
             CREATE TABLE failures (job_id TEXT NOT NULL, data TEXT NOT NULL);
             CREATE INDEX failures_by_job ON failures (job_id);
             CREATE TABLE stats_daily (
               account_id TEXT NOT NULL, day TEXT NOT NULL, deleted INTEGER NOT NULL DEFAULT 0,
               PRIMARY KEY (account_id, day)
             );
             CREATE TABLE rules (id TEXT PRIMARY KEY, account_id TEXT NOT NULL, data TEXT NOT NULL);
             CREATE TABLE packages (account_id TEXT PRIMARY KEY, data TEXT NOT NULL);
             CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             PRAGMA user_version = 1;
             COMMIT;",
        )?;
    }
    debug_assert_eq!(
        LATEST_SCHEMA, 1,
        "add a migration above when bumping the schema"
    );
    Ok(())
}
