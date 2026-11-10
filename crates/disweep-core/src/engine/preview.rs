use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::EngineError;
use super::spec::{JobSpec, Scope, Target};
use super::status::{Preview, SampleMessage, TargetPreview};
use crate::Snowflake;
use crate::api::{Discord, Message, SearchOutcome, SearchScope};
use crate::error::ApiError;
use crate::filter::{CompiledFilter, Verdict};
use crate::package::{self, PackageIndex};

const SAMPLE_PER_TARGET: usize = 12;
const SAMPLE_TOTAL: usize = 60;
const SAMPLE_CHARACTERS: usize = 400;
const HISTORY_SAMPLE_SIZE: u8 = 100;
const INDEX_WAIT_ATTEMPTS: u32 = 4;

const UNINDEXED_PAGES: u32 = 5;

pub(super) struct PreviewContext<'a> {
    pub client: &'a Discord,
    pub filter: &'a CompiledFilter,
    pub package: Option<&'a PackageIndex>,
    pub account: Snowflake,
    pub cancel: &'a CancellationToken,
}

struct Counted {
    count: u64,
    exact: bool,
    note: Option<String>,
    sample: Vec<Message>,
}

struct Unindexed {
    count: u64,

    complete: bool,
    sample: Vec<Message>,
}

impl PreviewContext<'_> {
    pub(super) async fn target(
        &self,
        target: &Target,
    ) -> Result<(TargetPreview, Vec<SampleMessage>), EngineError> {
        let counted = match target.scope {
            Scope::Package { channel_id } => self.count_package(channel_id).await?,
            Scope::Private { channel_id } => {
                let query = self.filter.search_query(false);
                match self
                    .count_search(SearchScope::Channel(channel_id), query)
                    .await
                {
                    Ok(Some(mut counted)) => {
                        counted.exact = counted.exact && self.filter.includes_pinned();
                        let newest_found = counted.sample.iter().map(|message| message.id).max();
                        let recent = self.count_unindexed(channel_id, newest_found).await?;
                        counted.count += recent.count;
                        counted.exact = counted.exact && recent.complete;
                        let mut sample = recent.sample;
                        sample.append(&mut counted.sample);
                        counted.sample = sample;
                        counted
                    }
                    Ok(None) | Err(SearchUnavailable) => self.count_history(channel_id).await?,
                    Err(SearchFailed(error)) => return Err(error),
                }
            }
            Scope::Channel {
                guild_id,
                channel_id,
            } => {
                let mut query = self.filter.search_query(true);
                query.channel_ids.push(channel_id);
                self.count_guild(guild_id, query).await?
            }
            Scope::Guild { guild_id } => {
                self.count_guild(guild_id, self.filter.search_query(true))
                    .await?
            }
        };

        let sample = counted
            .sample
            .iter()
            .filter(|message| self.filter.evaluate(message) == Verdict::Delete)
            .take(SAMPLE_PER_TARGET)
            .map(|message| SampleMessage {
                id: message.id,
                conversation: target.label.clone(),
                sent_at: message.id.timestamp_ms(),
                content: message.content.chars().take(SAMPLE_CHARACTERS).collect(),
                attachments: message.attachments.len() as u32,
            })
            .collect();
        let preview = TargetPreview {
            label: target.label.clone(),
            count: counted.count,
            exact: counted.exact,
            note: counted.note,
        };
        Ok((preview, sample))
    }

    async fn count_guild(
        &self,
        guild: Snowflake,
        query: crate::api::SearchQuery,
    ) -> Result<Counted, EngineError> {
        match self.count_search(SearchScope::Guild(guild), query).await {
            Ok(Some(counted)) => Ok(counted),
            Ok(None) => Ok(Counted {
                count: 0,
                exact: false,
                note: Some(
                    "Discord is still indexing this server, so the count isn't known yet."
                        .to_owned(),
                ),
                sample: Vec::new(),
            }),
            Err(SearchUnavailable) => Ok(Counted {
                count: 0,
                exact: false,
                note: Some("Disweep can't search this server.".to_owned()),
                sample: Vec::new(),
            }),
            Err(SearchFailed(error)) => Err(error),
        }
    }

    async fn count_search(
        &self,
        scope: SearchScope,
        query: crate::api::SearchQuery,
    ) -> Result<Option<Counted>, SearchProblem> {
        for attempt in 0..INDEX_WAIT_ATTEMPTS {
            let outcome = tokio::select! {
                biased;
                () = self.cancel.cancelled() => return Err(SearchFailed(EngineError::Cancelled)),
                outcome = self.client.search(scope, &query) => outcome,
            };
            match outcome {
                Ok(SearchOutcome::Ready(page)) => {
                    let matching = page
                        .hits
                        .iter()
                        .filter(|message| self.filter.evaluate(message) == Verdict::Delete)
                        .count();
                    let needs_estimate =
                        self.filter.needs_local_evaluation() && !page.hits.is_empty();
                    let count = if needs_estimate {
                        (page.total_results as f64 * matching as f64 / page.hits.len() as f64)
                            .round() as u64
                    } else {
                        page.total_results
                    };
                    return Ok(Some(Counted {
                        count,
                        exact: !needs_estimate,
                        note: None,
                        sample: page.hits,
                    }));
                }
                Ok(SearchOutcome::NotIndexed { retry_after }) => {
                    if attempt + 1 == INDEX_WAIT_ATTEMPTS {
                        return Ok(None);
                    }
                    tokio::select! {
                        biased;
                        () = self.cancel.cancelled() => return Err(SearchFailed(EngineError::Cancelled)),
                        () = tokio::time::sleep(retry_after.min(Duration::from_secs(8))) => {}
                    }
                }
                Err(ApiError::Discord {
                    status: 400 | 403 | 404,
                    ..
                }) => return Err(SearchUnavailable),
                Err(error) => return Err(SearchFailed(error.into())),
            }
        }
        Ok(None)
    }

    async fn count_unindexed(
        &self,
        channel: Snowflake,
        newest_found: Option<Snowflake>,
    ) -> Result<Unindexed, EngineError> {
        let passed = |id: Snowflake| {
            newest_found.is_some_and(|newest| id <= newest)
                || self.filter.history_floor().is_some_and(|floor| id < floor)
        };
        let mut before = self.filter.history_start();
        let mut unindexed = Unindexed {
            count: 0,
            complete: false,
            sample: Vec::new(),
        };
        for _ in 0..UNINDEXED_PAGES {
            let page = tokio::select! {
                biased;
                () = self.cancel.cancelled() => return Err(EngineError::Cancelled),
                page = self.client.messages(channel, before, HISTORY_SAMPLE_SIZE) => page,
            };
            let page = match page {
                Ok(page) => page,

                Err(ApiError::Discord {
                    status: 403 | 404, ..
                }) => return Ok(unindexed),
                Err(error) => return Err(error.into()),
            };
            let reached_end = page.len() < usize::from(HISTORY_SAMPLE_SIZE);
            before = page.last().map(|message| message.id);
            for message in page {
                if passed(message.id) {
                    unindexed.complete = true;
                    return Ok(unindexed);
                }
                if self.filter.evaluate(&message) == Verdict::Delete {
                    unindexed.count += 1;
                    unindexed.sample.push(message);
                }
            }
            if reached_end || before.is_none() {
                unindexed.complete = true;
                return Ok(unindexed);
            }
        }
        Ok(unindexed)
    }

    async fn count_history(&self, channel: Snowflake) -> Result<Counted, EngineError> {
        let messages = tokio::select! {
            biased;
            () = self.cancel.cancelled() => return Err(EngineError::Cancelled),
            messages = self.client.messages(channel, self.filter.history_start(), HISTORY_SAMPLE_SIZE) => messages,
        };
        match messages {
            Ok(messages) => {
                let count = messages
                    .iter()
                    .filter(|message| self.filter.evaluate(message) == Verdict::Delete)
                    .count() as u64;
                let is_whole_conversation = messages.len() < usize::from(HISTORY_SAMPLE_SIZE);
                let note = (!is_whole_conversation).then(|| {
                    "Discord's search wasn't available, so this counts only the most recent 100 messages.".to_owned()
                });
                Ok(Counted {
                    count,
                    exact: is_whole_conversation,
                    note,
                    sample: messages,
                })
            }
            Err(ApiError::Discord {
                status: 403 | 404, ..
            }) => Ok(Counted {
                count: 0,
                exact: false,
                note: Some("Disweep can't read this conversation.".to_owned()),
                sample: Vec::new(),
            }),
            Err(error) => Err(error.into()),
        }
    }

    async fn count_package(&self, channel_id: Snowflake) -> Result<Counted, EngineError> {
        let index = self.package.ok_or(EngineError::NoPackage)?;
        let channel = index
            .channels
            .iter()
            .find(|channel| channel.id == channel_id)
            .cloned()
            .ok_or(EngineError::NoPackage)?;
        let path = index.path.clone();
        let owner = self.account;
        let messages =
            tokio::task::spawn_blocking(move || package::read_channel(&path, &channel, owner))
                .await
                .map_err(|error| EngineError::Internal(error.to_string()))?
                .map_err(|error| EngineError::Package(error.to_string()))?;
        let matching: Vec<Message> = messages
            .into_iter()
            .filter(|message| self.filter.evaluate(message) == Verdict::Delete)
            .collect();
        Ok(Counted {
            count: matching.len() as u64,
            exact: true,
            note: None,
            sample: matching,
        })
    }
}

enum SearchProblem {
    SearchUnavailable,
    SearchFailed(EngineError),
}
use SearchProblem::{SearchFailed, SearchUnavailable};

pub(super) fn assemble(
    spec: &JobSpec,
    results: Vec<(TargetPreview, Vec<SampleMessage>)>,
) -> Preview {
    let mut targets = Vec::with_capacity(results.len());
    let mut sample = Vec::new();
    for (preview, mut messages) in results {
        targets.push(preview);
        sample.append(&mut messages);
    }
    sample.sort_by_key(|message| std::cmp::Reverse(message.sent_at));
    sample.truncate(SAMPLE_TOTAL);

    let total = targets.iter().map(|target| target.count).sum();
    let exact = targets.iter().all(|target| target.exact);
    let capped = spec.options.limit.map_or(total, |limit| limit.min(total));
    let estimated_ms =
        capped.saturating_mul(spec.options.speed.estimated_delete_cost().as_millis() as u64);
    Preview {
        targets,
        total: capped,
        exact,
        estimated_ms,
        sample,
    }
}
