use disweep_core::engine::{JobSnapshot, JobState, Preview, Rule, Stats};
use serde_json::{Value, json};

const EPHEMERAL: u64 = 1 << 6;
const BLUE: u32 = 0x4FA8FF;
const VIOLET: u32 = 0x8B7CF6;
const GREEN: u32 = 0x3DD68C;
const AMBER: u32 = 0xF2B24A;
const RED: u32 = 0xF2545B;

const BUTTON_DANGER: u8 = 4;
const BUTTON_SECONDARY: u8 = 2;

#[must_use]
pub fn count(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

#[must_use]
pub fn duration(milliseconds: u64) -> String {
    let seconds = milliseconds / 1000;
    match seconds {
        0..=59 => "under a minute".to_owned(),
        60..=3_599 => format!("about {} min", seconds.div_ceil(60)),
        3_600..=86_399 => format!("about {} h {} min", seconds / 3600, (seconds % 3600) / 60),
        _ => format!(
            "about {} d {} h",
            seconds / 86_400,
            (seconds % 86_400) / 3600
        ),
    }
}

fn embed(color: u32, title: &str, description: String) -> Value {
    json!({ "color": color, "title": title, "description": description })
}

fn private(embed: Value, components: Value) -> Value {
    json!({ "flags": EPHEMERAL, "content": "", "embeds": [embed], "components": components })
}

fn confirm_buttons(id: &str, label: &str) -> Value {
    json!([{ "type": 1, "components": [
        { "type": 2, "style": BUTTON_DANGER, "label": label, "custom_id": format!("ds:ok:{id}") },
        { "type": 2, "style": BUTTON_SECONDARY, "label": "Cancel", "custom_id": format!("ds:no:{id}") },
    ] }])
}

#[must_use]
pub fn notice(text: &str) -> Value {
    private(embed(VIOLET, "Disweep", text.to_owned()), json!([]))
}

#[must_use]
pub fn problem(text: &str) -> Value {
    private(embed(RED, "That didn't work", text.to_owned()), json!([]))
}

#[must_use]
pub fn nothing_to_sweep(target: &str) -> Value {
    private(
        embed(
            VIOLET,
            "Nothing to sweep",
            format!("None of your messages in **{target}** match."),
        ),
        json!([]),
    )
}

#[must_use]
pub fn sweep_preview(id: &str, target: &str, preview: &Preview) -> Value {
    let approximate = if preview.exact { "" } else { "about " };
    let mut description = format!(
        "**{approximate}{}** of your messages in **{target}** will be permanently deleted.\nEstimated time: {}.",
        count(preview.total),
        duration(preview.estimated_ms),
    );
    if let Some(note) = preview
        .targets
        .iter()
        .find_map(|target| target.note.as_deref())
    {
        description.push_str(&format!("\n\n{note}"));
    }
    description.push_str("\n\nThis can't be undone. Only you can see this message.");
    let label = format!("Delete {approximate}{}", count(preview.total));
    private(
        embed(AMBER, "Review this sweep", description),
        confirm_buttons(id, &label),
    )
}

#[must_use]
pub fn rule_preview(id: &str, target: &str, older_than: &str, every: &str) -> Value {
    let description = format!(
        "Every **{every}**, your messages in **{target}** older than **{older_than}** will be permanently deleted, for as long as Disweep is running.\n\nOnly you can see this message."
    );
    private(
        embed(AMBER, "Keep this chat clean?", description),
        confirm_buttons(id, "Turn on"),
    )
}

#[must_use]
pub fn purge_preview(id: &str, total: u64, too_old: u64, include_old: bool) -> Value {
    let mut description = format!(
        "**{}** messages in this channel will be permanently deleted.",
        count(total)
    );
    if too_old > 0 {
        if include_old {
            description.push_str(&format!(
                "\n\n{} of them are over two weeks old, so they go one at a time and take longer.",
                count(too_old)
            ));
        } else {
            description.push_str(&format!("\n\n{} more are over two weeks old and will be left alone. Turn on `include_old` to include them.", count(too_old)));
        }
    }
    let label = format!("Delete {}", count(total));
    private(
        embed(AMBER, "Review this purge", description),
        confirm_buttons(id, &label),
    )
}

#[must_use]
pub fn cancelled() -> Value {
    private(
        embed(VIOLET, "Cancelled", "Nothing was deleted.".to_owned()),
        json!([]),
    )
}

#[must_use]
pub fn expired() -> Value {
    private(
        embed(
            VIOLET,
            "This expired",
            "Run the command again to get a fresh preview.".to_owned(),
        ),
        json!([]),
    )
}

#[must_use]
pub fn purge_finished(deleted: u64, failed: u64) -> Value {
    let mut description = format!("Deleted **{}** messages.", count(deleted));
    if failed > 0 {
        description.push_str(&format!(" {} couldn't be deleted.", count(failed)));
    }
    private(embed(GREEN, "Purge finished", description), json!([]))
}

fn progress_bar(done: u64, total: u64) -> String {
    const WIDTH: u64 = 18;
    let filled = (done.min(total) * WIDTH).checked_div(total).unwrap_or(0);
    format!(
        "`{}{}`",
        "█".repeat(filled as usize),
        "░".repeat((WIDTH - filled) as usize)
    )
}

#[must_use]
pub fn job(snapshot: &JobSnapshot) -> Value {
    let counters = &snapshot.counters;
    let total = snapshot.total_estimate.unwrap_or(0);
    let (color, title) = match snapshot.state {
        JobState::Queued => (VIOLET, "Waiting its turn"),
        JobState::Running => (BLUE, "Sweeping"),
        JobState::Paused => (AMBER, "Paused"),
        JobState::Completed => (GREEN, "Sweep finished"),
        JobState::Stopped => (AMBER, "Sweep stopped"),
        JobState::Failed => (RED, "Sweep failed"),
        JobState::Interrupted => (AMBER, "Sweep interrupted"),
    };

    let mut description = format!("**{}**\n", snapshot.label);
    if snapshot.state.is_active() {
        description.push_str(&format!(
            "{} {} of {}\n",
            progress_bar(counters.deleted, total),
            count(counters.deleted),
            count(total)
        ));
        if let Some(eta) = snapshot
            .eta_ms
            .filter(|_| snapshot.state == JobState::Running)
        {
            description.push_str(&format!("Time left: {}\n", duration(eta)));
        }
        if snapshot.rate_limited_until.is_some() {
            description.push_str("Discord asked for a pause, so Disweep is waiting.\n");
        }
    } else {
        description.push_str(&format!("Deleted **{}**", count(counters.deleted)));
        if counters.skipped > 0 {
            description.push_str(&format!(" · kept {}", count(counters.skipped)));
        }
        if counters.failed > 0 {
            description.push_str(&format!(" · failed {}", count(counters.failed)));
        }
        description.push('\n');
    }
    if let Some(failure) = &snapshot.failure {
        description.push_str(&format!("\n{}\n", failure.message));
    }
    private(embed(color, title, description), json!([]))
}

#[must_use]
pub fn still_running(snapshot: &JobSnapshot) -> Value {
    let mut view = job(snapshot);
    view["embeds"][0]["footer"] = json!({ "text": "Discord stops updates after 15 minutes. Use /sweep status to check on it." });
    view
}

#[must_use]
pub fn stats(stats: &Stats) -> Value {
    let last_week: u64 = stats.days.iter().rev().take(7).map(|day| day.deleted).sum();
    let description = format!(
        "Deleted in total: **{}**\nDeleted in the last 7 active days: **{}**\nSweeps finished: **{}**",
        count(stats.total_deleted),
        count(last_week),
        count(stats.jobs_finished),
    );
    private(embed(VIOLET, "Your sweeping", description), json!([]))
}

#[must_use]
pub fn rules(rules: &[Rule]) -> Value {
    if rules.is_empty() {
        return notice("You have no automatic sweeps. Use `/autosweep set` in a chat to add one.");
    }
    let lines: Vec<String> = rules
        .iter()
        .take(20)
        .map(|rule| {
            let state = if rule.enabled { "on" } else { "off" };
            format!(
                "**{}** · older than {} · {state}",
                rule.label,
                duration_short(rule.older_than_secs)
            )
        })
        .collect();
    private(
        embed(VIOLET, "Automatic sweeps", lines.join("\n")),
        json!([]),
    )
}

#[must_use]
pub fn duration_short(seconds: u64) -> String {
    for (unit, size) in [
        ("y", 31_536_000),
        ("mo", 2_592_000),
        ("w", 604_800),
        ("d", 86_400),
        ("h", 3_600),
    ] {
        if seconds >= size && seconds.is_multiple_of(size) {
            return format!("{}{unit}", seconds / size);
        }
    }
    format!("{}m", seconds.div_ceil(60))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_group_digits() {
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1_000), "1,000");
        assert_eq!(count(1_234_567), "1,234,567");
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(duration(30_000), "under a minute");
        assert_eq!(duration(90_000), "about 2 min");
        assert_eq!(duration(3_660_000), "about 1 h 1 min");
        assert_eq!(duration(90_000_000), "about 1 d 1 h");
    }

    #[test]
    fn short_durations_pick_the_largest_exact_unit() {
        assert_eq!(duration_short(2_592_000), "1mo");
        assert_eq!(duration_short(86_400 * 7), "1w");
        assert_eq!(duration_short(86_400 * 3), "3d");
        assert_eq!(duration_short(3_600), "1h");
        assert_eq!(duration_short(90), "2m");
    }

    #[test]
    fn every_reply_is_private() {
        assert_eq!(notice("x")["flags"], EPHEMERAL);
        assert_eq!(problem("x")["flags"], EPHEMERAL);
        assert_eq!(cancelled()["flags"], EPHEMERAL);
    }

    #[test]
    fn the_bar_fills_with_progress() {
        assert_eq!(progress_bar(0, 100).matches('█').count(), 0);
        assert_eq!(progress_bar(50, 100).matches('█').count(), 9);
        assert_eq!(progress_bar(100, 100).matches('█').count(), 18);
        assert_eq!(progress_bar(5, 0).matches('█').count(), 0);
    }
}
