use std::time::Duration;

use tokio::time::{Instant, sleep_until};

use super::spec::Speed;

const CEILING: Duration = Duration::from_secs(30);
const SUCCESSES_BEFORE_SPEEDING_UP: u32 = 15;

impl Speed {
    fn delete_start(self) -> Duration {
        Duration::from_millis(match self {
            Self::Careful => 2_000,
            Self::Balanced => 1_400,
            Self::Fast => 1_000,
        })
    }

    fn delete_floor(self) -> Duration {
        Duration::from_millis(match self {
            Self::Careful => 1_600,
            Self::Balanced => 1_100,
            Self::Fast => 750,
        })
    }

    #[must_use]
    pub fn search_interval(self) -> Duration {
        Duration::from_millis(match self {
            Self::Careful => 4_000,
            Self::Balanced => 2_500,
            Self::Fast => 1_500,
        })
    }

    #[must_use]
    pub fn history_interval(self) -> Duration {
        Duration::from_millis(match self {
            Self::Careful => 1_500,
            Self::Balanced => 1_000,
            Self::Fast => 500,
        })
    }

    #[must_use]
    pub fn estimated_delete_cost(self) -> Duration {
        self.delete_start() + self.search_interval() / 25
    }
}

pub struct Pacer {
    interval: Duration,
    floor: Duration,
    next_at: Option<Instant>,
    successes: u32,
}

impl Pacer {
    #[must_use]
    pub fn new(speed: Speed) -> Self {
        Self::scaled(speed, 1.0)
    }

    #[must_use]
    pub fn scaled(speed: Speed, scale: f64) -> Self {
        Self {
            interval: speed.delete_start().mul_f64(scale),
            floor: speed.delete_floor().mul_f64(scale),
            next_at: None,
            successes: 0,
        }
    }

    #[must_use]
    pub fn interval(&self) -> Duration {
        self.interval
    }

    pub async fn wait(&mut self) {
        if let Some(next_at) = self.next_at {
            sleep_until(next_at).await;
        }
        self.next_at = Some(Instant::now() + self.interval);
    }

    pub fn on_success(&mut self) {
        self.successes += 1;
        if self.successes >= SUCCESSES_BEFORE_SPEEDING_UP {
            self.successes = 0;
            self.interval = self.interval.mul_f64(0.95).max(self.floor);
        }
    }

    pub fn on_throttled(&mut self, wait: Duration) {
        self.successes = 0;
        self.interval = self.interval.mul_f64(1.5).min(CEILING);
        self.next_at = Some(Instant::now() + wait.max(self.interval));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn spaces_deletes_by_the_interval() {
        let mut pacer = Pacer::new(Speed::Balanced);
        let started = Instant::now();
        pacer.wait().await;
        assert_eq!(
            started.elapsed(),
            Duration::ZERO,
            "the first delete doesn't wait"
        );
        pacer.wait().await;
        assert_eq!(started.elapsed(), Speed::Balanced.delete_start());
    }

    #[tokio::test(start_paused = true)]
    async fn a_429_slows_down_and_waits_at_least_as_long_as_asked() {
        let mut pacer = Pacer::new(Speed::Balanced);
        pacer.wait().await;
        let before = pacer.interval();
        pacer.on_throttled(Duration::from_secs(20));
        assert!(pacer.interval() > before);

        let started = Instant::now();
        pacer.wait().await;
        assert_eq!(started.elapsed(), Duration::from_secs(20));
    }

    #[test]
    fn eases_back_after_a_clean_run_but_never_below_the_floor() {
        let mut pacer = Pacer::new(Speed::Balanced);
        for _ in 0..5_000 {
            pacer.on_success();
        }
        assert_eq!(pacer.interval(), Speed::Balanced.delete_floor());
    }

    #[test]
    fn backing_off_has_a_ceiling() {
        let mut pacer = Pacer::new(Speed::Careful);
        for _ in 0..100 {
            pacer.on_throttled(Duration::ZERO);
        }
        assert_eq!(pacer.interval(), CEILING);
    }

    #[test]
    fn a_429_resets_the_clean_run() {
        let mut pacer = Pacer::new(Speed::Balanced);
        for _ in 0..(SUCCESSES_BEFORE_SPEEDING_UP - 1) {
            pacer.on_success();
        }
        pacer.on_throttled(Duration::ZERO);
        let slowed = pacer.interval();
        pacer.on_success();
        assert_eq!(pacer.interval(), slowed);
    }
}
