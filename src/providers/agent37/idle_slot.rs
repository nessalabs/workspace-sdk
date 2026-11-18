//! One value kept while it is used, and dropped once it has gone unused
//! for an idle time, whether or not anything asks for it again.
use std::{
    future::Future,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::sync::Mutex;

struct Slot<T> {
    value: Option<T>,
    /// Bumped on every use; a reaper drops the value only if no use came
    /// after the one that started it.
    generation: u64,
}

/// A value shared while it is in use. Each use starts its own reaper,
/// which drops the value after the idle time unless a later use came.
pub(super) struct IdleSlot<T> {
    slot: Arc<Mutex<Slot<T>>>,
    idle: Duration,
}
impl<T: Clone + Send + 'static> IdleSlot<T> {
    pub fn new(idle: Duration) -> Self {
        Self {
            slot: Arc::new(Mutex::new(Slot {
                value: None,
                generation: 0,
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
        slot.generation += 1;
        let generation = slot.generation;
        let weak: Weak<Mutex<Slot<T>>> = Arc::downgrade(&self.slot);
        let idle = self.idle;
        tokio::spawn(async move {
            tokio::time::sleep(idle).await;
            if let Some(slot) = weak.upgrade() {
                let mut slot = slot.lock().await;
                if slot.generation == generation {
                    slot.value = None;
                }
            }
        });
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
