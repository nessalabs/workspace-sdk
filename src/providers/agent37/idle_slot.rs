//! One value kept while it is used, and dropped once it has gone unused
//! for an idle time, whether or not anything asks for it again.
use std::{
    future::Future,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::{sync::Mutex, time::Instant};

struct Slot<T> {
    value: Option<T>,
    /// When the value goes, unless a use moves it later.
    deadline: Instant,
    /// The one reaper waiting on the deadline. One that ended, or died
    /// with its runtime, is replaced on the next use.
    reaper: Option<tokio::task::JoinHandle<()>>,
}

/// A value shared while it is in use. One reaper waits on a deadline each
/// use moves on, and drops the value once the deadline passes unmoved.
pub(super) struct IdleSlot<T> {
    slot: Arc<Mutex<Slot<T>>>,
    idle: Duration,
}
impl<T: Clone + Send + 'static> IdleSlot<T> {
    pub fn new(idle: Duration) -> Self {
        Self {
            slot: Arc::new(Mutex::new(Slot {
                value: None,
                deadline: Instant::now(),
                reaper: None,
            })),
            idle,
        }
    }
    /// The value kept, if `usable` says it still is, or a new one `make`
    /// makes. The slot is held while `make` runs, so concurrent callers
    /// wait for it rather than make another.
    pub async fn get_or_make<E, F>(
        &self,
        usable: impl Fn(&T) -> bool,
        make: impl FnOnce() -> F,
    ) -> Result<T, E>
    where
        F: Future<Output = Result<T, E>>,
    {
        let mut slot = self.slot.lock().await;
        let value = match slot.value.take().filter(|value| usable(value)) {
            Some(value) => value,
            None => make().await?,
        };
        slot.value = Some(value.clone());
        self.stamp(&mut slot);
        Ok(value)
    }
    /// Count a use that just ended, so the idle time runs from now.
    pub async fn touch(&self) {
        let mut slot = self.slot.lock().await;
        if slot.value.is_some() {
            self.stamp(&mut slot);
        }
    }
    fn stamp(&self, slot: &mut Slot<T>) {
        slot.deadline = Instant::now() + self.idle;
        if slot
            .reaper
            .as_ref()
            .is_some_and(|reaper| !reaper.is_finished())
        {
            return;
        }
        let weak: Weak<Mutex<Slot<T>>> = Arc::downgrade(&self.slot);
        let mut deadline = slot.deadline;
        slot.reaper = Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep_until(deadline).await;
                let Some(slot) = weak.upgrade() else { return };
                let mut slot = slot.lock().await;
                if slot.deadline <= Instant::now() {
                    slot.value = None;
                    // Gone under the lock, so a use that comes next never
                    // sees this reaper as still waiting.
                    slot.reaper = None;
                    return;
                }
                deadline = slot.deadline;
            }
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn a_value_unused_for_the_idle_time_is_dropped_unasked() {
        let slot = IdleSlot::new(Duration::from_secs(120));
        let value = Arc::new(());
        let made = slot
            .get_or_make(|_| true, || async { Ok::<_, ()>(value.clone()) })
            .await
            .unwrap();
        drop(made);
        tokio::time::sleep(Duration::from_secs(100)).await;
        // A use ending restarts the idle time.
        slot.touch().await;
        tokio::time::sleep(Duration::from_secs(100)).await;
        assert_eq!(Arc::strong_count(&value), 2, "still kept after 100 s idle");
        tokio::time::sleep(Duration::from_secs(30)).await;
        tokio::task::yield_now().await;
        assert_eq!(Arc::strong_count(&value), 1, "dropped with no request");
        // A later value gets a reaper of its own.
        slot.get_or_make(|_| true, || async { Ok::<_, ()>(value.clone()) })
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(121)).await;
        tokio::task::yield_now().await;
        assert_eq!(Arc::strong_count(&value), 1, "dropped again");
    }

    #[tokio::test(start_paused = true)]
    async fn many_uses_share_one_reaper() {
        let slot = IdleSlot::new(Duration::from_secs(120));
        let value = Arc::new(());
        slot.get_or_make(|_| true, || async { Ok::<_, ()>(value.clone()) })
            .await
            .unwrap();
        let before = tokio::runtime::Handle::current()
            .metrics()
            .num_alive_tasks();
        for _ in 0..50 {
            slot.touch().await;
        }
        let after = tokio::runtime::Handle::current()
            .metrics()
            .num_alive_tasks();
        assert_eq!(before, after, "touches start no tasks");
        // The one reaper still drops the value once the last touch is old.
        tokio::time::sleep(Duration::from_secs(121)).await;
        tokio::task::yield_now().await;
        assert_eq!(Arc::strong_count(&value), 1, "dropped by the one reaper");
    }

    #[tokio::test(start_paused = true)]
    async fn a_kept_value_is_reused_and_an_unusable_one_replaced() {
        let slot = IdleSlot::new(Duration::from_secs(120));
        let first = slot
            .get_or_make(|_| true, || async { Ok::<_, ()>(1) })
            .await
            .unwrap();
        let again = slot
            .get_or_make(|_| true, || async { Ok::<_, ()>(2) })
            .await
            .unwrap();
        assert_eq!((first, again), (1, 1));
        let replaced = slot
            .get_or_make(|_| false, || async { Ok::<_, ()>(3) })
            .await
            .unwrap();
        assert_eq!(replaced, 3);
        assert_eq!(
            slot.get_or_make(|_| false, || async { Err::<i32, _>("no") })
                .await,
            Err("no")
        );
        // A failed make leaves nothing kept.
        assert_eq!(
            slot.get_or_make(|_| true, || async { Ok::<_, ()>(4) })
                .await,
            Ok(4)
        );
    }
}
