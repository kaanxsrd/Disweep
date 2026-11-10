#![allow(clippy::unwrap_used)]

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use disweep_core::api::Method;
use disweep_core::demo::{DEMO_TOKEN, FakeDiscord, Faults};
use disweep_core::engine::{
    ArchiveOptions, Engine, EngineError, EngineEvent, FailureKind, JobOptions, JobOrigin,
    JobSnapshot, JobSpec, JobState, RuleDraft, Scope, Speed, Strategy, Target, TargetState,
};
use disweep_core::filter::{Filters, HasKind};
use disweep_core::store::Store;
use disweep_core::{Secret, Snowflake, now_ms};
use tokio_util::sync::CancellationToken;

const ME: (u64, &str) = (1, "Kit");
const OTHER: (u64, &str) = (2, "Ada");
const DM: u64 = 50;
const GUILD: u64 = 60;
const CHANNEL: u64 = 61;

fn id(n: u64) -> u64 {
    (1_000 + n) << 22
}

async fn setup(fake: FakeDiscord) -> (Engine, Arc<FakeDiscord>, Snowflake) {
    disweep_core::init_tls();
    let fake = Arc::new(fake);
    fake.set_me(ME.0, ME.1);
    fake.add_dm(DM, OTHER.0, OTHER.1);
    fake.add_guild(GUILD, "Rust Harbor");
    fake.add_guild_channel(GUILD, CHANNEL, "general");
    let engine = Engine::new(
        Store::in_memory().unwrap(),
        fake.clone(),
        std::env::temp_dir().join("disweep-tests"),
    );
    let account = engine
        .sign_in(&Secret::new(DEMO_TOKEN.to_owned()))
        .await
        .unwrap();
    (engine, fake, account.id)
}

fn dm_target() -> Target {
    Target {
        scope: Scope::Private {
            channel_id: Snowflake(DM),
        },
        label: "Ada".to_owned(),
        expected: None,
    }
}

fn channel_target() -> Target {
    Target {
        scope: Scope::Channel {
            guild_id: Snowflake(GUILD),
            channel_id: Snowflake(CHANNEL),
        },
        label: "#general".to_owned(),
        expected: None,
    }
}

fn spec(account: Snowflake, targets: Vec<Target>) -> JobSpec {
    JobSpec {
        account_id: account,
        targets,
        filters: Filters::default(),
        options: JobOptions::default(),
        origin: JobOrigin::App,
    }
}

fn with_strategy(mut job: JobSpec, strategy: Strategy) -> JobSpec {
    job.options.strategy = strategy;
    job
}

async fn run(engine: &Engine, job: JobSpec) -> JobSnapshot {
    let started = engine.start(job).await.unwrap();
    wait_for_end(engine, &started.id).await
}

async fn wait_for_end(engine: &Engine, id: &str) -> JobSnapshot {
    for _ in 0..4_000_000 {
        if let Some(snapshot) = engine.snapshot(id)
            && !snapshot.state.is_active()
        {
            return snapshot;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the job never finished");
}

fn seed(fake: &FakeDiscord, channel: u64, range: std::ops::Range<u64>) {
    for n in range {
        let author = if n % 2 == 0 { ME } else { OTHER };
        fake.add_message(channel, id(n), author, &format!("message {n}"));
    }
}

#[tokio::test(start_paused = true)]
async fn deletes_only_your_messages_from_a_dm() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    seed(&fake, DM, 0..250);

    let done = run(&engine, spec(me, vec![dm_target()])).await;

    assert_eq!(done.state, JobState::Completed);
    assert_eq!(done.counters.deleted, 125);
    let survivors = fake.remaining(DM);
    assert_eq!(survivors.len(), 125);
    assert!(
        survivors.iter().all(|message| (message >> 22) % 2 == 1),
        "only the other person's messages remain"
    );
}

#[tokio::test(start_paused = true)]
async fn search_results_are_never_trusted_about_who_wrote_something() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    fake.add_message(CHANNEL, id(1), OTHER, "not yours");
    fake.add_message(CHANNEL, id(2), ME, "yours");
    fake.set_faults(Faults {
        search_ignores_author: true,
        ..Faults::default()
    });

    let done = run(&engine, spec(me, vec![channel_target()])).await;

    assert_eq!(done.counters.deleted, 1);
    assert_eq!(fake.remaining(CHANNEL), vec![id(1)]);
    assert_eq!(
        fake.count_requests(&Method::DELETE),
        1,
        "no delete was even attempted on the other message"
    );
}

#[tokio::test(start_paused = true)]
async fn a_preview_never_deletes_anything() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    seed(&fake, CHANNEL, 0..40);

    let preview = engine
        .preview(
            "p".to_owned(),
            spec(me, vec![channel_target()]),
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(preview.total, 20);
    assert!(preview.exact);
    assert!(!preview.sample.is_empty());
    assert_eq!(fake.count_requests(&Method::DELETE), 0);
    assert_eq!(fake.count_requests(&Method::POST), 0);
    assert_eq!(fake.remaining(CHANNEL).len(), 40);
}

#[tokio::test(start_paused = true)]
async fn a_dm_count_is_only_an_estimate_while_pins_are_kept() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    fake.add_message(DM, id(1), ME, "x");
    fake.add_pinned(DM, id(2), ME, "keep");

    let keeping = engine
        .preview(
            "p".to_owned(),
            spec(me, vec![dm_target()]),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!keeping.exact);

    let mut job = spec(me, vec![dm_target()]);
    job.filters.include_pinned = true;
    let including = engine
        .preview("p".to_owned(), job, CancellationToken::new())
        .await
        .unwrap();
    assert!(including.exact);
    assert_eq!(including.total, 2);
}

#[tokio::test(start_paused = true)]
async fn a_dm_preview_counts_new_messages_search_has_not_indexed_yet() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..17 {
        fake.add_message(DM, id(n), ME, "test");
    }
    fake.add_message(DM, id(17), OTHER, "theirs");
    fake.set_faults(Faults {
        hidden_from_search: (9..17).map(id).collect(),
        ..Faults::default()
    });
    let mut job = spec(me, vec![dm_target()]);
    job.filters.include_pinned = true;

    let preview = engine
        .preview("p".to_owned(), job.clone(), CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(preview.total, 17);
    assert!(preview.exact);
    assert!(
        preview
            .sample
            .iter()
            .any(|message| message.id == Snowflake(id(16))),
        "the newest, unindexed message is in the sample"
    );
    assert_eq!(fake.count_requests(&Method::DELETE), 0);
    let done = run(&engine, job).await;
    assert_eq!(
        done.counters.deleted, preview.total,
        "the sweep matched the preview"
    );
}

#[tokio::test(start_paused = true)]
async fn a_dm_preview_reads_back_only_as_far_as_search_reached() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    seed(&fake, DM, 0..1_000);
    let mut job = spec(me, vec![dm_target()]);
    job.filters.include_pinned = true;

    let preview = engine
        .preview("p".to_owned(), job, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(preview.total, 500, "nothing was counted twice");
    assert!(preview.exact);

    assert_eq!(fake.count_requests(&Method::GET), 3);
}

#[tokio::test(start_paused = true)]
async fn a_preview_estimates_when_a_filter_only_works_locally() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..50 {
        let text = if n % 2 == 0 {
            "keep this"
        } else {
            "remove this"
        };
        fake.add_message(CHANNEL, id(n), ME, text);
    }
    let mut job = spec(me, vec![channel_target()]);
    job.filters.pattern = Some("^remove".to_owned());

    let preview = engine
        .preview("p".to_owned(), job, CancellationToken::new())
        .await
        .unwrap();

    assert!(!preview.exact);
    assert!((20..=30).contains(&preview.total), "got {}", preview.total);
    assert!(
        preview
            .sample
            .iter()
            .all(|message| message.content.starts_with("remove"))
    );
}

#[tokio::test(start_paused = true)]
async fn a_preview_can_be_cancelled() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    fake.set_faults(Faults {
        latency: Duration::from_secs(5),
        ..Faults::default()
    });
    let cancel = CancellationToken::new();
    cancel.cancel();
    let result = engine
        .preview("p".to_owned(), spec(me, vec![channel_target()]), cancel)
        .await;
    assert!(matches!(result, Err(EngineError::Cancelled)));
}

#[tokio::test(start_paused = true)]
async fn sweeps_past_discords_ten_thousand_result_search_limit() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..10_400 {
        fake.add_message(CHANNEL, id(n), ME, "x");
    }

    let done = run(&engine, spec(me, vec![channel_target()])).await;

    assert_eq!(done.counters.deleted, 10_400);
    assert!(fake.remaining(CHANNEL).is_empty());
    let searches = fake
        .requests()
        .into_iter()
        .filter(|r| r.path.ends_with("/search"));
    assert!(
        searches
            .into_iter()
            .all(|r| r.query.iter().all(|(key, _)| key != "offset")),
        "cursors, never offsets"
    );
}

#[tokio::test(start_paused = true)]
async fn lagging_search_results_do_not_cause_repeat_deletes() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    seed(&fake, CHANNEL, 0..120);
    fake.set_faults(Faults {
        stale_search: true,
        ..Faults::default()
    });

    let done = run(&engine, spec(me, vec![channel_target()])).await;

    assert_eq!(done.counters.deleted, 60);
    assert_eq!(done.counters.already_gone, 0);
    assert_eq!(fake.count_requests(&Method::DELETE), 60);
}

#[tokio::test(start_paused = true)]
async fn waits_for_an_index_that_is_still_building() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    seed(&fake, CHANNEL, 0..10);
    fake.set_faults(Faults {
        index_pending: 3,
        ..Faults::default()
    });

    let done = run(&engine, spec(me, vec![channel_target()])).await;

    assert_eq!(done.state, JobState::Completed);
    assert_eq!(done.counters.deleted, 5);
}

#[tokio::test(start_paused = true)]
async fn an_empty_page_with_results_left_is_retried_instead_of_ending() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    seed(&fake, CHANNEL, 0..20);
    fake.set_faults(Faults {
        empty_search_pages: 2,
        ..Faults::default()
    });

    let done = run(&engine, spec(me, vec![channel_target()])).await;

    assert_eq!(done.counters.deleted, 10);
}

#[tokio::test(start_paused = true)]
async fn thorough_mode_finds_what_search_never_returned() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..10 {
        fake.add_message(CHANNEL, id(n), ME, "x");
    }
    let hidden: HashSet<u64> = [id(3), id(7)].into_iter().collect();
    fake.set_faults(Faults {
        hidden_from_search: hidden,
        ..Faults::default()
    });

    let mut job = with_strategy(spec(me, vec![channel_target()]), Strategy::Search);
    job.options.thorough = true;
    let done = run(&engine, job).await;

    assert_eq!(done.counters.deleted, 10);
    assert!(fake.remaining(CHANNEL).is_empty());
}

#[tokio::test(start_paused = true)]
async fn without_thorough_mode_search_gaps_remain() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..10 {
        fake.add_message(CHANNEL, id(n), ME, "x");
    }
    let hidden: HashSet<u64> = [id(3)].into_iter().collect();
    fake.set_faults(Faults {
        hidden_from_search: hidden,
        ..Faults::default()
    });

    let done = run(
        &engine,
        with_strategy(spec(me, vec![channel_target()]), Strategy::Search),
    )
    .await;

    assert_eq!(done.counters.deleted, 9);
    assert_eq!(fake.remaining(CHANNEL), vec![id(3)]);
}

#[tokio::test(start_paused = true)]
async fn a_dm_falls_back_to_history_when_search_is_unavailable() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..10 {
        fake.add_message(DM, id(n), ME, "needle");
    }
    fake.set_faults(Faults {
        dm_search_missing: true,
        ..Faults::default()
    });

    let mut job = spec(me, vec![dm_target()]);
    job.filters.contains = Some("needle".to_owned());
    let done = run(&engine, job).await;

    assert_eq!(done.counters.deleted, 10);
    assert!(done.notes.iter().any(|note| note.contains("full history")));
}

#[tokio::test(start_paused = true)]
async fn auto_walks_dms_and_searches_servers() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    fake.add_message(DM, id(1), ME, "x");
    fake.add_message(CHANNEL, id(2), ME, "x");

    run(&engine, spec(me, vec![dm_target(), channel_target()])).await;

    let paths: Vec<String> = fake
        .requests()
        .into_iter()
        .filter(|r| r.method == Method::GET)
        .map(|r| r.path)
        .collect();
    assert!(
        paths
            .iter()
            .any(|p| p == &format!("/channels/{DM}/messages"))
    );
    assert!(
        !paths
            .iter()
            .any(|p| p == &format!("/channels/{DM}/messages/search"))
    );
    assert!(
        paths
            .iter()
            .any(|p| p == &format!("/guilds/{GUILD}/messages/search"))
    );
}

#[tokio::test(start_paused = true)]
async fn pinned_and_system_messages_are_kept() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    fake.add_pinned(DM, id(1), ME, "important");
    fake.add_system(DM, id(2), ME, 3);
    fake.add_message(DM, id(3), ME, "normal");

    let done = run(&engine, spec(me, vec![dm_target()])).await;

    assert_eq!(done.counters.deleted, 1);
    assert_eq!(done.counters.skipped, 2);
    assert_eq!(fake.remaining(DM), vec![id(1), id(2)]);
    assert_eq!(
        fake.count_requests(&Method::DELETE),
        1,
        "no request is wasted on what can't be deleted"
    );
}

#[tokio::test(start_paused = true)]
async fn pinned_messages_go_when_included() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    fake.add_pinned(DM, id(1), ME, "important");
    let mut job = spec(me, vec![dm_target()]);
    job.filters.include_pinned = true;

    assert_eq!(run(&engine, job).await.counters.deleted, 1);
}

#[tokio::test(start_paused = true)]
async fn the_date_range_is_respected() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    let moment = 1_700_000_000_000;
    let early = Snowflake::from_timestamp_ms(moment - 1000).0;
    let inside = Snowflake::from_timestamp_ms(moment + 1000).0;
    let late = Snowflake::from_timestamp_ms(moment + 5000).0;
    fake.add_message(DM, early, ME, "early");
    fake.add_message(DM, inside, ME, "inside");
    fake.add_message(DM, late, ME, "late");

    let mut job = spec(me, vec![dm_target()]);
    job.filters.after = Some(moment);
    job.filters.before = Some(moment + 2000);
    let done = run(&engine, job).await;

    assert_eq!(done.counters.deleted, 1);
    assert_eq!(fake.remaining(DM), vec![early, late]);
}

#[tokio::test(start_paused = true)]
async fn has_filters_select_the_right_messages() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    fake.add_message(CHANNEL, id(1), ME, "plain");
    fake.add_message(CHANNEL, id(2), ME, "see https://example.com");
    let mut job = spec(me, vec![channel_target()]);
    job.filters.has = vec![HasKind::Link];

    assert_eq!(run(&engine, job).await.counters.deleted, 1);
    assert_eq!(fake.remaining(CHANNEL), vec![id(1)]);
}

#[tokio::test(start_paused = true)]
async fn the_limit_stops_the_sweep() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..50 {
        fake.add_message(DM, id(n), ME, "x");
    }
    let mut job = spec(me, vec![dm_target()]);
    job.options.limit = Some(7);

    let done = run(&engine, job).await;

    assert_eq!(done.state, JobState::Completed);
    assert_eq!(done.counters.deleted, 7);
    assert_eq!(fake.remaining(DM).len(), 43);
}

#[tokio::test(start_paused = true)]
async fn the_newest_messages_go_first() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..5 {
        fake.add_message(DM, id(n), ME, "x");
    }
    let mut job = spec(me, vec![dm_target()]);
    job.options.limit = Some(2);
    run(&engine, job).await;
    assert_eq!(fake.remaining(DM), vec![id(0), id(1), id(2)]);
}

#[tokio::test(start_paused = true)]
async fn rate_limits_are_honoured_and_reported() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..30 {
        fake.add_message(DM, id(n), ME, "x");
    }
    fake.set_faults(Faults {
        rate_limit_every: Some(10),
        rate_limit_seconds: 5.0,
        ..Faults::default()
    });

    let done = run(&engine, spec(me, vec![dm_target()])).await;

    assert_eq!(done.counters.deleted, 30);
    assert!(done.counters.throttle_count >= 3);
    assert!(done.counters.throttled_ms >= 15_000);
    assert!(
        done.pacing_ms > 1_400,
        "the pace slowed down after being limited, got {}",
        done.pacing_ms
    );
}

#[tokio::test(start_paused = true)]
async fn one_refused_message_does_not_stop_the_rest() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..6 {
        fake.add_message(DM, id(n), ME, "x");
    }
    let refused: HashMap<u64, u32> = [(id(3), 50083)].into_iter().collect();
    fake.set_faults(Faults {
        refuse_delete: refused,
        ..Faults::default()
    });

    let done = run(&engine, spec(me, vec![dm_target()])).await;

    assert_eq!(done.state, JobState::Completed);
    assert_eq!(done.counters.deleted, 5);
    assert_eq!(done.counters.failed, 1);
    let failures = engine.failures(&done.id).await.unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].code, Some(50083));
}

#[tokio::test(start_paused = true)]
async fn an_inaccessible_conversation_is_skipped_and_the_next_one_still_runs() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    fake.add_message(DM, id(1), ME, "x");
    fake.add_message(CHANNEL, id(2), ME, "x");
    let denied: HashSet<u64> = [DM].into_iter().collect();
    fake.set_faults(Faults {
        denied_channels: denied,
        ..Faults::default()
    });

    let done = run(&engine, spec(me, vec![dm_target(), channel_target()])).await;

    assert_eq!(done.state, JobState::Completed);
    assert_eq!(done.targets[0].state, TargetState::Skipped);
    assert_eq!(done.targets[1].state, TargetState::Done);
    assert_eq!(done.counters.deleted, 1);
}

#[tokio::test(start_paused = true)]
async fn losing_access_midway_stops_asking_about_that_channel() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..5 {
        fake.add_message(CHANNEL, id(n), ME, "x");
    }
    let denied: HashSet<u64> = [CHANNEL].into_iter().collect();
    fake.set_faults(Faults {
        denied_channels: denied,
        ..Faults::default()
    });

    let done = run(&engine, spec(me, vec![channel_target()])).await;

    assert_eq!(
        fake.count_requests(&Method::DELETE),
        1,
        "one refusal is enough to know"
    );
    assert_eq!(done.counters.failed, 1);
    assert_eq!(done.counters.skipped, 4);
}

#[tokio::test(start_paused = true)]
async fn a_dead_token_stops_the_job_with_a_clear_reason() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    fake.add_message(DM, id(1), ME, "x");
    fake.set_faults(Faults {
        reject_token: true,
        ..Faults::default()
    });

    let done = run(&engine, spec(me, vec![dm_target()])).await;

    assert_eq!(done.state, JobState::Failed);
    assert_eq!(done.failure.unwrap().kind, FailureKind::Login);
}

#[tokio::test(start_paused = true)]
async fn pausing_holds_and_resuming_finishes() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..200 {
        fake.add_message(DM, id(n), ME, "x");
    }
    let started = engine.start(spec(me, vec![dm_target()])).await.unwrap();
    tokio::time::sleep(Duration::from_secs(8)).await;

    engine.pause(&started.id).unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let held = fake.count_requests(&Method::DELETE);
    assert!(held > 0, "some were deleted before pausing");
    assert_eq!(
        engine.snapshot(&started.id).unwrap().state,
        JobState::Paused
    );

    tokio::time::sleep(Duration::from_secs(120)).await;
    assert_eq!(
        fake.count_requests(&Method::DELETE),
        held,
        "nothing is deleted while paused"
    );

    engine.resume(&started.id).unwrap();
    let done = wait_for_end(&engine, &started.id).await;
    assert_eq!(done.state, JobState::Completed);
    assert_eq!(done.counters.deleted, 200);
}

#[tokio::test(start_paused = true)]
async fn stopping_ends_the_job_and_it_can_be_resumed() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..300 {
        fake.add_message(DM, id(n), ME, "x");
    }
    let started = engine.start(spec(me, vec![dm_target()])).await.unwrap();

    tokio::time::sleep(Duration::from_secs(200)).await;

    engine.stop(&started.id).unwrap();
    let stopped = wait_for_end(&engine, &started.id).await;
    assert_eq!(stopped.state, JobState::Stopped);
    assert!(stopped.counters.deleted < 300);
    let deleted_before = stopped.counters.deleted;

    engine.resume_finished(&started.id).await.unwrap();
    let done = wait_for_end(&engine, &started.id).await;

    assert_eq!(done.state, JobState::Completed);
    assert_eq!(
        done.counters.deleted, 300,
        "progress carried over from {deleted_before}"
    );
    assert!(fake.remaining(DM).is_empty());
}

#[tokio::test(start_paused = true)]
async fn jobs_on_one_account_run_one_at_a_time_in_order() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..30 {
        fake.add_message(DM, id(n), ME, "x");
    }
    fake.add_message(CHANNEL, id(100), ME, "x");

    let first = engine.start(spec(me, vec![dm_target()])).await.unwrap();
    let second = engine
        .start(spec(me, vec![channel_target()]))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;

    assert_eq!(engine.snapshot(&first.id).unwrap().state, JobState::Running);
    assert_eq!(engine.snapshot(&second.id).unwrap().state, JobState::Queued);

    wait_for_end(&engine, &first.id).await;
    assert_eq!(
        wait_for_end(&engine, &second.id).await.state,
        JobState::Completed
    );
}

#[tokio::test(start_paused = true)]
async fn an_interrupted_job_is_found_at_startup_and_resumes_from_its_checkpoint() {
    let store = Store::in_memory().unwrap();
    let fake = Arc::new(FakeDiscord::new());
    fake.set_me(ME.0, ME.1);
    fake.add_dm(DM, OTHER.0, OTHER.1);
    for n in 0..300 {
        fake.add_message(DM, id(n), ME, "x");
    }

    let first_run = Engine::new(store.clone(), fake.clone(), std::env::temp_dir());
    let me = first_run
        .sign_in(&Secret::new(DEMO_TOKEN.to_owned()))
        .await
        .unwrap()
        .id;
    let job = first_run.start(spec(me, vec![dm_target()])).await.unwrap();
    tokio::time::sleep(Duration::from_secs(200)).await;
    let reached = first_run.snapshot(&job.id).unwrap().counters.deleted;
    assert!(reached > 0);

    first_run.stop_all();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let mut saved = store.job(job.id.clone()).await.unwrap().unwrap();
    saved.snapshot.state = JobState::Running;
    store.save_job(saved).await.unwrap();

    let second_run = Engine::new(store.clone(), fake.clone(), std::env::temp_dir());
    second_run
        .sign_in(&Secret::new(DEMO_TOKEN.to_owned()))
        .await
        .unwrap();

    assert_eq!(second_run.recover().await.unwrap(), 1);
    assert_eq!(
        store
            .job(job.id.clone())
            .await
            .unwrap()
            .unwrap()
            .snapshot
            .state,
        JobState::Interrupted
    );

    second_run.resume_finished(&job.id).await.unwrap();
    let done = wait_for_end(&second_run, &job.id).await;
    assert_eq!(done.state, JobState::Completed);
    assert!(fake.remaining(DM).is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_finished_job_cannot_be_resumed() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    fake.add_message(DM, id(1), ME, "x");
    let done = run(&engine, spec(me, vec![dm_target()])).await;
    assert!(matches!(
        engine.resume_finished(&done.id).await,
        Err(EngineError::NotResumable)
    ));
}

#[tokio::test(start_paused = true)]
async fn the_archive_is_written_before_deleting() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    fake.add_message(DM, id(1), ME, "keep a copy of this");
    fake.add_attachment(
        DM,
        id(1),
        "pic.png",
        "https://cdn.discordapp.com/attachments/1/2/pic.png",
    );
    let root = tempfile::tempdir().unwrap();
    let engine = {
        drop(engine);
        let engine = Engine::new(
            Store::in_memory().unwrap(),
            fake.clone(),
            root.path().to_path_buf(),
        );
        engine
            .sign_in(&Secret::new(DEMO_TOKEN.to_owned()))
            .await
            .unwrap();
        engine
    };

    let mut job = spec(me, vec![dm_target()]);
    job.options.archive = ArchiveOptions {
        enabled: true,
        attachments: true,
    };
    let done = run(&engine, job).await;

    assert_eq!(done.counters.deleted, 1);
    let folder = std::path::PathBuf::from(done.archive_path.unwrap());
    assert!(folder.starts_with(root.path()));
    let copy = std::fs::read_to_string(folder.join("messages.jsonl")).unwrap();
    assert!(copy.contains("keep a copy of this"));
    assert!(folder.join("transcript.html").exists());
    assert_eq!(
        std::fs::read_dir(folder.join("attachments"))
            .unwrap()
            .count(),
        1
    );
}

#[tokio::test(start_paused = true)]
async fn an_unwritable_archive_stops_the_job_before_anything_is_deleted() {
    let (_, fake, _) = setup(FakeDiscord::new()).await;
    fake.add_message(DM, id(1), ME, "x");
    let root = tempfile::tempdir().unwrap();

    let blocker = root.path().join("blocked");
    std::fs::write(&blocker, "file").unwrap();
    let engine = Engine::new(Store::in_memory().unwrap(), fake.clone(), blocker);
    let me = engine
        .sign_in(&Secret::new(DEMO_TOKEN.to_owned()))
        .await
        .unwrap()
        .id;

    let mut job = spec(me, vec![dm_target()]);
    job.options.archive = ArchiveOptions {
        enabled: true,
        attachments: false,
    };
    let done = run(&engine, job).await;

    assert_eq!(done.state, JobState::Failed);
    assert_eq!(done.failure.unwrap().kind, FailureKind::Archive);
    assert_eq!(fake.count_requests(&Method::DELETE), 0);
}

fn package_zip(owner: u64, channel: u64, ids: &[u64]) -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("package.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("Account/user.json", options).unwrap();
    write!(zip, r#"{{"id": "{owner}", "username": "kit"}}"#).unwrap();
    zip.start_file(format!("Messages/c{channel}/channel.json"), options)
        .unwrap();
    write!(zip, r#"{{"id": "{channel}", "type": 1}}"#).unwrap();
    zip.start_file(format!("Messages/c{channel}/messages.json"), options)
        .unwrap();
    let rows: Vec<String> = ids
        .iter()
        .map(|id| format!(r#"{{"ID": {id}, "Contents": "m", "Attachments": ""}}"#))
        .collect();
    write!(zip, "[{}]", rows.join(",")).unwrap();
    zip.finish().unwrap();
    (directory, path)
}

fn package_target() -> Target {
    Target {
        scope: Scope::Package {
            channel_id: Snowflake(DM),
        },
        label: "Closed DM".to_owned(),
        expected: None,
    }
}

#[tokio::test(start_paused = true)]
async fn a_data_package_reaches_messages_directly_by_id() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..150 {
        fake.add_message(DM, id(n), ME, "m");
    }
    let ids: Vec<u64> = (0..150).map(id).collect();
    let (_guard, path) = package_zip(ME.0, DM, &ids);

    let index = engine.import_package(me, path).await.unwrap();
    assert_eq!(index.total_messages, 150);

    let preview = engine
        .preview(
            "p".to_owned(),
            spec(me, vec![package_target()]),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(preview.total, 150);
    assert!(preview.exact);

    let done = run(&engine, spec(me, vec![package_target()])).await;

    assert_eq!(done.counters.deleted, 150);
    assert!(fake.remaining(DM).is_empty());
    assert!(
        fake.requests().iter().all(|r| !r.path.ends_with("/search")),
        "a package needs no search"
    );
}

#[tokio::test(start_paused = true)]
async fn someone_elses_data_package_is_refused() {
    let (engine, _fake, me) = setup(FakeDiscord::new()).await;
    let (_guard, path) = package_zip(999, DM, &[id(1)]);
    assert!(matches!(
        engine.import_package(me, path).await,
        Err(EngineError::PackageMismatch)
    ));
}

#[tokio::test(start_paused = true)]
async fn a_package_that_does_not_say_whose_it_is_is_refused() {
    let (engine, _fake, me) = setup(FakeDiscord::new()).await;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("package.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("Messages/c50/channel.json", options)
        .unwrap();
    write!(zip, r#"{{"id": "50", "type": 1}}"#).unwrap();
    zip.start_file("Messages/c50/messages.json", options)
        .unwrap();
    write!(zip, r#"[{{"ID": 5, "Contents": "m"}}]"#).unwrap();
    zip.finish().unwrap();

    assert!(matches!(
        engine.import_package(me, path).await,
        Err(EngineError::PackageMismatch)
    ));
}

#[tokio::test(start_paused = true)]
async fn a_job_paused_while_queued_starts_paused() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..30 {
        fake.add_message(DM, id(n), ME, "x");
    }
    fake.add_message(CHANNEL, id(100), ME, "x");
    let first = engine.start(spec(me, vec![dm_target()])).await.unwrap();
    let second = engine
        .start(spec(me, vec![channel_target()]))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    engine.pause(&second.id).unwrap();

    wait_for_end(&engine, &first.id).await;
    tokio::time::sleep(Duration::from_secs(5)).await;

    assert_eq!(engine.snapshot(&second.id).unwrap().state, JobState::Paused);
    assert_eq!(fake.remaining(CHANNEL).len(), 1);
    engine.resume(&second.id).unwrap();
    assert_eq!(
        wait_for_end(&engine, &second.id).await.state,
        JobState::Completed
    );
}

#[tokio::test(start_paused = true)]
async fn messages_already_gone_are_counted_separately() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    let (_guard, path) = package_zip(ME.0, DM, &[id(1), id(2)]);
    fake.add_message(DM, id(2), ME, "m");
    engine.import_package(me, path).await.unwrap();

    let done = run(&engine, spec(me, vec![package_target()])).await;

    assert_eq!(done.counters.deleted, 1);
    assert_eq!(done.counters.already_gone, 1);
    assert_eq!(done.counters.failed, 0);
}

fn rule_draft(me: Snowflake, older_than_secs: u64) -> RuleDraft {
    RuleDraft {
        account_id: me,
        label: "Keep Ada's DM tidy".to_owned(),
        targets: vec![dm_target()],
        older_than_secs,
        every_secs: 3_600,
        filters: Filters::default(),
    }
}

#[tokio::test(start_paused = true)]
async fn rules_delete_only_what_is_older_than_the_cutoff() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    let now = now_ms();
    let old = Snowflake::from_timestamp_ms(now - 40 * 86_400_000).0;
    let recent = Snowflake::from_timestamp_ms(now - 86_400_000).0;
    fake.add_message(DM, old, ME, "old");
    fake.add_message(DM, recent, ME, "recent");

    let rule = engine.add_rule(rule_draft(me, 30 * 86_400)).await.unwrap();
    let job = engine.run_rule(&rule.id).await.unwrap();
    let done = wait_for_end(&engine, &job.id).await;

    assert_eq!(done.counters.deleted, 1);
    assert_eq!(done.origin, JobOrigin::Rule { rule_id: rule.id });
    assert_eq!(fake.remaining(DM), vec![recent]);
}

#[tokio::test(start_paused = true)]
async fn due_rules_run_once_and_then_wait_for_their_interval() {
    let (engine, _fake, me) = setup(FakeDiscord::new()).await;
    engine.add_rule(rule_draft(me, 60)).await.unwrap();
    let now = now_ms();

    assert_eq!(engine.run_due_rules(now).await.unwrap(), 1);
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert_eq!(engine.run_due_rules(now + 1_000).await.unwrap(), 0);
    assert_eq!(engine.run_due_rules(now + 3_601_000).await.unwrap(), 1);
}

#[tokio::test(start_paused = true)]
async fn disabled_rules_do_not_run() {
    let (engine, _fake, me) = setup(FakeDiscord::new()).await;
    let rule = engine.add_rule(rule_draft(me, 60)).await.unwrap();
    engine.set_rule_enabled(&rule.id, false).await.unwrap();
    assert_eq!(
        engine.run_due_rules(now_ms() + 10_000_000).await.unwrap(),
        0
    );
}

#[tokio::test(start_paused = true)]
async fn a_rule_can_not_run_too_often() {
    let (engine, _fake, me) = setup(FakeDiscord::new()).await;
    let mut draft = rule_draft(me, 60);
    draft.every_secs = 60;
    assert!(matches!(
        engine.add_rule(draft).await,
        Err(EngineError::BadRuleTiming)
    ));
}

#[tokio::test(start_paused = true)]
async fn statistics_add_up_across_jobs() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..4 {
        fake.add_message(DM, id(n), ME, "x");
    }
    let done = run(&engine, spec(me, vec![dm_target()])).await;
    assert_eq!(done.counters.deleted, 4);

    let stats = engine.stats(me).await.unwrap();
    assert_eq!(stats.total_deleted, 4);
    assert_eq!(stats.jobs_finished, 1);
    assert_eq!(stats.days.iter().map(|day| day.deleted).sum::<u64>(), 4);
}

#[tokio::test(start_paused = true)]
async fn progress_is_published_and_the_estimate_tracks_reality() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..30 {
        fake.add_message(CHANNEL, id(n), ME, "x");
    }
    let mut events = engine.subscribe();
    let done = run(&engine, spec(me, vec![channel_target()])).await;

    let (mut feed_items, mut job_events) = (0, 0);
    while let Ok(event) = events.try_recv() {
        match event {
            EngineEvent::Feed { items, .. } => feed_items += items.len(),
            EngineEvent::Job { .. } => job_events += 1,
            EngineEvent::PreviewProgress { .. } => {}
        }
    }
    assert!(job_events >= 2);
    assert_eq!(feed_items, 30);
    assert_eq!(done.total_estimate, Some(30));
}

#[tokio::test(start_paused = true)]
async fn invalid_specs_are_refused() {
    let (engine, _fake, me) = setup(FakeDiscord::new()).await;
    assert!(matches!(
        engine.start(spec(me, vec![])).await,
        Err(EngineError::NoTargets)
    ));

    let mut bad_pattern = spec(me, vec![dm_target()]);
    bad_pattern.filters.pattern = Some("(".to_owned());
    assert!(matches!(
        engine.start(bad_pattern).await,
        Err(EngineError::Filter(_))
    ));

    assert!(matches!(
        engine.start(spec(Snowflake(404), vec![dm_target()])).await,
        Err(EngineError::UnknownAccount)
    ));
}

#[tokio::test(start_paused = true)]
async fn signing_out_stops_sweeps_and_erases_the_account() {
    let (engine, fake, me) = setup(FakeDiscord::new()).await;
    for n in 0..100 {
        fake.add_message(DM, id(n), ME, "x");
    }
    let job = engine.start(spec(me, vec![dm_target()])).await.unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;

    engine.sign_out(me).await.unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;

    assert_eq!(engine.snapshot(&job.id).unwrap().state, JobState::Stopped);
    assert!(engine.accounts().is_empty());
    assert!(engine.store().accounts().await.unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_bot_token_is_turned_away_at_sign_in() {
    disweep_core::init_tls();
    let fake = Arc::new(FakeDiscord::new());
    fake.state_set_bot();
    let engine = Engine::new(Store::in_memory().unwrap(), fake, std::env::temp_dir());
    assert!(matches!(
        engine.sign_in(&Secret::new(DEMO_TOKEN.to_owned())).await,
        Err(EngineError::BotAccount)
    ));
}

#[tokio::test(start_paused = true)]
async fn the_demo_data_works_end_to_end() {
    disweep_core::init_tls();
    let fake = Arc::new(FakeDiscord::demo());
    fake.set_faults(Faults::default());
    let engine = Engine::new(
        Store::in_memory().unwrap(),
        fake.clone(),
        std::env::temp_dir(),
    );
    let me = engine
        .sign_in(&Secret::new(DEMO_TOKEN.to_owned()))
        .await
        .unwrap()
        .id;

    let inventory = engine.inventory(me).await.unwrap();
    assert_eq!(inventory.conversations.len(), 6);
    assert_eq!(inventory.guilds.len(), 3);
    assert!(
        !engine
            .guild_channels(me, inventory.guilds[0].id)
            .await
            .unwrap()
            .is_empty()
    );

    let mut job = spec(
        me,
        vec![Target {
            scope: Scope::Private {
                channel_id: inventory.conversations[0].id,
            },
            label: "first".to_owned(),
            expected: None,
        }],
    );
    job.options.speed = Speed::Fast;
    let done = run(&engine, job).await;
    assert_eq!(done.state, JobState::Completed);
    assert!(done.counters.deleted > 0);
}
