//! Named uses of anything a caller keeps awake, and the turn that orders
//! opening it against resting it.
use std::{
    collections::{BTreeMap, HashMap},
    hash::Hash,
    sync::{Arc, Mutex as SyncMutex, MutexGuard, PoisonError},
    time::Duration,
};
use tokio::{
    sync::{Mutex, Notify, OwnedMutexGuard},
    time::Instant,
};

/// Who uses each thing a caller keeps awake, by name, and since when nothing
/// has. A thing is in use exactly while at least one [`Use`] for it is alive;
/// nothing reads reference counts.
///
/// [`Machines`](crate::Machines) uses it for machines this SDK saved. A
/// caller that keeps its own records of what it runs (a VM it made through a
/// provider, say) uses it directly, keyed by its own ids:
///
/// - Opening and resting each take the key's [`Turn`], so a rest never stops
///   what an opening is about to hand out.
/// - Anything that uses the thing holds a [`Use`] named for what it is: a
///   lease, a command, an opening. Taking one never waits, so it can be taken
///   while another lock is held.
/// - A rest takes the turn, sees nothing in use and the idle time passed, and
///   stops the thing, then says so with [`Turn::rested`].
///
/// Keep one `Uses` per set of things: a second one does not see these uses.
pub struct Uses<K> {
    entries: SyncMutex<HashMap<K, Arc<Entry>>>,
}

struct Entry {
    /// Orders opening for use against resting.
    turn: Arc<Mutex<()>>,
    holders: SyncMutex<Holders>,
    /// Woken each time the last use ends.
    freed: Notify,
}
#[derive(Default)]
struct Holders {
    next: u64,
    names: BTreeMap<u64, String>,
    /// When the last use ended, or a wake began. `None` while in use, before
    /// any use, and once rested.
    idle_since: Option<Instant>,
}

/// One named use. Dropping it ends the use; the last one to end starts the
/// thing's idle time.
#[must_use = "the use ends as soon as it is dropped"]
pub struct Use {
    entry: Arc<Entry>,
    id: u64,
}
impl Drop for Use {
    fn drop(&mut self) {
        // Ending a use only ever moves a thing toward rest, so it needs no
        // ordering with opening or resting beyond this short lock.
        let mut holders = lock(&self.entry.holders);
        holders.names.remove(&self.id);
        if holders.names.is_empty() {
            holders.idle_since = Some(Instant::now());
            drop(holders);
            self.entry.freed.notify_waiters();
        }
    }
}
impl std::fmt::Debug for Use {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let holders = lock(&self.entry.holders);
        f.debug_struct("Use")
            .field("name", &holders.names.get(&self.id))
            .finish()
    }
}

/// The one turn at a key: while it is held, nothing else opens or rests that
/// thing. Uses can still be taken and ended meanwhile.
#[must_use = "the turn ends as soon as it is dropped"]
pub struct Turn {
    entry: Arc<Entry>,
    _turn: OwnedMutexGuard<()>,
}
impl Turn {
    /// Whether anything uses the thing now.
    pub fn in_use(&self) -> bool {
        !lock(&self.entry.holders).names.is_empty()
    }
    /// How long nothing has used the thing: `None` while in use, before any
    /// use or wake, and once rested.
    pub fn idle_for(&self) -> Option<Duration> {
        let holders = lock(&self.entry.holders);
        if !holders.names.is_empty() {
            return None;
        }
        holders.idle_since.map(|since| since.elapsed())
    }
    /// Whether a rest for `idle` is due: nothing uses it, and nothing has
    /// for at least `idle`.
    pub fn due(&self, idle: Duration) -> bool {
        self.idle_for().is_some_and(|elapsed| elapsed >= idle)
    }
    /// Say the thing may be awake from now, before any use of it is taken:
    /// a wake that fails, panics or is cancelled part way is then still
    /// rested once idle. An idle time already running is kept, so failing
    /// wakes cannot push a rest back.
    pub fn waking(&self) {
        lock(&self.entry.holders)
            .idle_since
            .get_or_insert_with(Instant::now);
    }
    /// Say the thing was stopped: it is no longer due for rest until it is
    /// used or woken again.
    pub fn rested(self) {
        let mut holders = lock(&self.entry.holders);
        if holders.names.is_empty() {
            holders.idle_since = None;
        }
    }
    /// Start a named use, as [`Uses::hold`] does.
    pub fn hold(&self, name: impl Into<String>) -> Use {
        hold(&self.entry, name.into())
    }
}

impl<K: Eq + Hash + Clone> Default for Uses<K> {
    fn default() -> Self {
        Self {
            entries: SyncMutex::new(HashMap::new()),
        }
    }
}
impl<K: Eq + Hash + Clone> Uses<K> {
    /// No uses yet.
    pub fn new() -> Self {
        Self::default()
    }
    /// Start a named use of `key`. It never waits: the caller says the thing
    /// is open, and a rest that takes the turn after this sees it in use.
    pub fn hold(&self, key: &K, name: impl Into<String>) -> Use {
        hold(&self.entry(key), name.into())
    }
    /// Wait for `key`'s turn, to open or rest it.
    pub async fn turn(&self, key: &K) -> Turn {
        let entry = self.entry(key);
        let turn = entry.turn.clone().lock_owned().await;
        Turn { entry, _turn: turn }
    }
    /// `key`'s turn if nothing holds it now: a rest pass skips a thing being
    /// opened, since it is about to be used.
    pub fn try_turn(&self, key: &K) -> Option<Turn> {
        let entry = self.entry(key);
        let turn = entry.turn.clone().try_lock_owned().ok()?;
        Some(Turn { entry, _turn: turn })
    }
    /// The names of `key`'s uses now.
    pub fn uses(&self, key: &K) -> Vec<String> {
        let Some(entry) = lock(&self.entries).get(key).cloned() else {
            return Vec::new();
        };
        let holders = lock(&entry.holders);
        holders.names.values().cloned().collect()
    }
    /// Whether anything uses `key` now.
    pub fn in_use(&self, key: &K) -> bool {
        lock(&self.entries)
            .get(key)
            .is_some_and(|entry| !lock(&entry.holders).names.is_empty())
    }
    /// Answers once nothing uses `key`: at once if nothing does now. A use
    /// taken after it answers is the caller's to see, with the key's
    /// [`Turn`].
    pub async fn released(&self, key: &K) {
        let entry = self.entry(key);
        loop {
            let freed = entry.freed.notified();
            tokio::pin!(freed);
            // Registered before looking, so an end between the look and the
            // wait is not missed.
            freed.as_mut().enable();
            if lock(&entry.holders).names.is_empty() {
                return;
            }
            freed.await;
        }
    }
    /// Every key nothing has used for at least `idle`, and that has not been
    /// rested since: the ones a rest pass takes the turn of.
    pub fn idle(&self, idle: Duration) -> Vec<K> {
        lock(&self.entries)
            .iter()
            .filter(|(_, entry)| {
                let holders = lock(&entry.holders);
                holders.names.is_empty()
                    && holders
                        .idle_since
                        .is_some_and(|since| since.elapsed() >= idle)
            })
            .map(|(key, _)| key.clone())
            .collect()
    }
    fn entry(&self, key: &K) -> Arc<Entry> {
        lock(&self.entries)
            .entry(key.clone())
            .or_insert_with(|| {
                Arc::new(Entry {
                    turn: Arc::new(Mutex::new(())),
                    holders: SyncMutex::new(Holders::default()),
                    freed: Notify::new(),
                })
            })
            .clone()
    }
}

fn hold(entry: &Arc<Entry>, name: String) -> Use {
    let mut holders = lock(&entry.holders);
    let id = holders.next;
    holders.next += 1;
    holders.names.insert(id, name);
    holders.idle_since = None;
    Use {
        entry: entry.clone(),
        id,
    }
}

/// Take a short lock. Every change under these locks is a single step, so a
/// panic elsewhere cannot leave them half-changed.
fn lock<T>(mutex: &SyncMutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
