//! Finding candidate messages, one page at a time.
//!
//! Scanners move a cursor backwards through message IDs instead of using a search `offset`.
//! Deleting shifts every offset, Discord caps offsets at 9,975, and its index lags behind deletes,
//! which together are why offset-based tools stop early with thousands of messages left.

use std::time::Duration;

use tokio::time::{Instant, sleep, sleep_until};

use crate::Snowflake;
use crate::api::{Discord, Message, SearchOutcome, SearchQuery, SearchScope};
use crate::error::{ApiError, ApiResult};

const MAX_INDEX_WAITS: u32 = 12;
const MAX_EMPTY_RETRIES: u32 = 3;
const EMPTY_RETRY_DELAY: Duration = Duration::from_secs(4);
const HISTORY_PAGE_SIZE: u8 = 100;
const PACKAGE_PAGE_SIZE: usize = 100;

/// One batch of candidates, newest first.
#[derive(Debug)]
pub struct Page {
    pub messages: Vec<Message>,
    /// Discord's count of what is still to come, including this page, when it gives one.
    pub remaining: Option<u64>,
}

pub enum Scanner {
    Search(SearchScanner),
    History(HistoryScanner),
    Package(PackageScanner),
}

impl Scanner {
    /// The next page, or `None` when there is nothing more to find.
    pub async fn next_page(&mut self) -> ApiResult<Option<Page>> {
        match self {
            Self::Search(scanner) => scanner.next_page().await,
            Self::History(scanner) => scanner.next_page().await,
            Self::Package(scanner) => Ok(scanner.next_page()),
        }
    }

    /// Where to pick up after a restart.
    #[must_use]
    pub fn cursor(&self) -> Option<Snowflake> {
        match self {
            Self::Search(scanner) => scanner.cursor,
            Self::History(scanner) => scanner.cursor,
            Self::Package(scanner) => scanner.cursor(),
        }
    }

    /// How many results Discord still claimed to have when search gave up, if it did.
    #[must_use]
    pub fn stalled_with(&self) -> Option<u64> {
        match self {
            Self::Search(scanner) => scanner.stalled_with,
            _ => None,
        }
    }
}

pub struct SearchScanner {
    client: Discord,
    scope: SearchScope,
    query: SearchQuery,
    cursor: Option<Snowflake>,
    min_interval: Duration,
    next_request: Option<Instant>,
    finished: bool,
    stalled_with: Option<u64>,
}

impl SearchScanner {
    #[must_use]
    pub fn new(
        client: Discord,
        scope: SearchScope,
        query: SearchQuery,
        cursor: Option<Snowflake>,
        min_interval: Duration,
    ) -> Self {
        Self {
            client,
            scope,
            query,
            cursor,
            min_interval,
            next_request: None,
            finished: false,
            stalled_with: None,
        }
    }

    async fn next_page(&mut self) -> ApiResult<Option<Page>> {
        if self.finished {
            return Ok(None);
        }
        let mut index_waits = 0;
        let mut empty_retries = 0;

        loop {
            if let Some(next_request) = self.next_request {
                sleep_until(next_request).await;
            }
            self.next_request = Some(Instant::now() + self.min_interval);

            let mut query = self.query.clone();
            query.max_id = self.cursor.or(query.max_id);

            match self.client.search(self.scope, &query).await? {
                SearchOutcome::NotIndexed { retry_after } => {
                    index_waits += 1;
                    if index_waits > MAX_INDEX_WAITS {
                        return Err(ApiError::IndexUnavailable);
                    }
                    sleep(retry_after).await;
                }
                SearchOutcome::Ready(page) if page.hits.is_empty() => {
                    // Discord says not to treat a short page as the end, and an empty one with
                    // results still counted is usually the index catching up.
                    if (page.total_results > 0 || page.deep_indexing)
                        && empty_retries < MAX_EMPTY_RETRIES
                    {
                        empty_retries += 1;
                        sleep(EMPTY_RETRY_DELAY * empty_retries).await;
                        continue;
                    }
                    self.finished = true;
                    self.stalled_with = (page.total_results > 0).then_some(page.total_results);
                    return Ok(None);
                }
                SearchOutcome::Ready(page) => {
                    let oldest = page.hits.iter().map(|message| message.id).min();
                    // A cursor that doesn't move would loop forever.
                    if self.cursor.is_some() && oldest >= self.cursor {
                        self.finished = true;
                        return Ok(None);
                    }
                    self.cursor = oldest;
                    return Ok(Some(Page {
                        messages: page.hits,
                        remaining: Some(page.total_results),
                    }));
                }
            }
        }
    }
}

pub struct HistoryScanner {
    client: Discord,
    channel: Snowflake,
    cursor: Option<Snowflake>,
    floor: Option<Snowflake>,
    min_interval: Duration,
    next_request: Option<Instant>,
    finished: bool,
}

impl HistoryScanner {
    #[must_use]
    pub fn new(
        client: Discord,
        channel: Snowflake,
        cursor: Option<Snowflake>,
        floor: Option<Snowflake>,
        min_interval: Duration,
    ) -> Self {
        Self {
            client,
            channel,
            cursor,
            floor,
            min_interval,
            next_request: None,
            finished: false,
        }
    }

    async fn next_page(&mut self) -> ApiResult<Option<Page>> {
        if self.finished {
            return Ok(None);
        }
        if let Some(next_request) = self.next_request {
            sleep_until(next_request).await;
        }
        self.next_request = Some(Instant::now() + self.min_interval);

        let messages = self
            .client
            .messages(self.channel, self.cursor, HISTORY_PAGE_SIZE)
            .await?;
        let Some(oldest) = messages.iter().map(|message| message.id).min() else {
            self.finished = true;
            return Ok(None);
        };
        self.cursor = Some(oldest);
        if self.floor.is_some_and(|floor| oldest <= floor) {
            self.finished = true;
        }
        Ok(Some(Page {
            messages,
            remaining: None,
        }))
    }
}

/// Walks messages listed in a Discord data package.
pub struct PackageScanner {
    messages: Vec<Message>,
    position: usize,
}

impl PackageScanner {
    /// `messages` must be sorted newest first. Anything at or above `cursor` was already handled.
    #[must_use]
    pub fn new(messages: Vec<Message>, cursor: Option<Snowflake>) -> Self {
        let position = cursor.map_or(0, |cursor| {
            messages.partition_point(|message| message.id >= cursor)
        });
        Self { messages, position }
    }

    fn next_page(&mut self) -> Option<Page> {
        let end = (self.position + PACKAGE_PAGE_SIZE).min(self.messages.len());
        if self.position >= end {
            return None;
        }
        let remaining = (self.messages.len() - self.position) as u64;
        let page = self.messages[self.position..end].to_vec();
        self.position = end;
        Some(Page {
            messages: page,
            remaining: Some(remaining),
        })
    }

    fn cursor(&self) -> Option<Snowflake> {
        self.position
            .checked_sub(1)
            .map(|index| self.messages[index].id)
    }
}
