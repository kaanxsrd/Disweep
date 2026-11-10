use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stopped;

pub struct Control {
    paused: watch::Sender<bool>,
    cancel: CancellationToken,
}

impl Control {
    #[must_use]
    pub fn new() -> Self {
        Self {
            paused: watch::Sender::new(false),
            cancel: CancellationToken::new(),
        }
    }

    pub fn pause(&self) {
        self.paused.send_replace(true);
    }

    pub fn resume(&self) {
        self.paused.send_replace(false);
    }

    pub fn stop(&self) {
        self.cancel.cancel();
    }

    #[must_use]
    pub fn is_paused(&self) -> bool {
        *self.paused.borrow()
    }

    pub async fn stopped(&self) {
        self.cancel.cancelled().await;
    }

    pub async fn proceed(&self) -> Result<(), Stopped> {
        let mut paused = self.paused.subscribe();
        loop {
            if self.cancel.is_cancelled() {
                return Err(Stopped);
            }
            if !*paused.borrow_and_update() {
                return Ok(());
            }
            tokio::select! {
                _ = paused.changed() => {}
                () = self.cancel.cancelled() => return Err(Stopped),
            }
        }
    }
}

impl Default for Control {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn proceeds_immediately_when_running() {
        assert_eq!(Control::new().proceed().await, Ok(()));
    }

    #[tokio::test(start_paused = true)]
    async fn a_pause_holds_until_resumed() {
        let control = Arc::new(Control::new());
        control.pause();
        let waiter = tokio::spawn({
            let control = control.clone();
            async move { control.proceed().await }
        });
        tokio::time::sleep(Duration::from_secs(60)).await;
        assert!(!waiter.is_finished());

        control.resume();
        assert_eq!(waiter.await.unwrap(), Ok(()));
    }

    #[tokio::test(start_paused = true)]
    async fn stopping_releases_a_paused_job() {
        let control = Arc::new(Control::new());
        control.pause();
        let waiter = tokio::spawn({
            let control = control.clone();
            async move { control.proceed().await }
        });
        tokio::time::sleep(Duration::from_secs(1)).await;

        control.stop();
        assert_eq!(waiter.await.unwrap(), Err(Stopped));
    }
}
