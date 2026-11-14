//! Named uses of anything a caller keeps awake, and the turn that orders
//! opening it against resting it.
use std::{
    collections::{BTreeMap, HashMap},
    hash::Hash,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex as SyncMutex, MutexGuard, PoisonError},
    time::Duration,
};
use tokio::{
    sync::{Mutex, Notify, OwnedMutexGuard},
    time::Instant,
};

/// Who uses each thing a caller keeps awake, by name, and since when nothing
/// has, with what the caller keeps about each thing (`S`) behind its turn.
///
/// [`Machines`](crate::Machines) uses it for machines this SDK saved. A
/// caller that keeps its own records of what it runs (a VM it made through a
/// provider, say) uses it directly, keyed by its own ids:
///
/// - Opening and resting each take the key's [`UseTurn`], which alone
///   reaches `S`. A thing's first use is taken under its turn
///   ([`UseTurn::hold`]), so a rest, which takes the same turn, never stops
///   what an opening is about to hand out.
/// - A use already held can be extended ([`NamedUse::also`]) without the
///   turn: the thing cannot rest while the use it extends is alive. That is
///   how a lease or a command started over an open thing is counted.
/// - A rest takes the turn, sees nothing in use and the idle time passed
///   ([`UseTurn::due`]), stops the thing, then says so with
///   [`Uses::rested`], which forgets the thing once nothing refers to it.
///
/// A thing is in use exactly while at least one [`NamedUse`] for it is
/// alive. Waiting for a turn may be cancelled at any point: nothing changes
/// until the turn is had.
///
/// It keeps one small entry for each thing used or turned to since it was
/// last rested; [`Uses::len`] says how many. Keep one `Uses` per set of
/// things: a second one does not see these uses.
pub struct Uses<K, S = ()> {
    entries: SyncMutex<HashMap<K, Arc<Entry<S>>>>,
}

struct Entry<S> {
    /// Orders opening for use against resting, and holds the caller's state.
    turn: Arc<Mutex<S>>,
    shared: Arc<Shared>,
}
struct Shared {
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
impl Holders {
    /// How long nothing has used the thing, if it is idle at all.
    fn idle_for(&self) -> Option<Duration> {
        if !self.names.is_empty() {
            return None;
        }
        self.idle_since.map(|since| since.elapsed())
    }
    fn due(&self, idle: Duration) -> bool {
        self.idle_for().is_some_and(|elapsed| elapsed >= idle)
    }
    fn start(&mut self, shared: &Arc<Shared>, name: String) -> NamedUse {
        let id = self.next;
        self.next += 1;
        self.names.insert(id, name);
        self.idle_since = None;
        NamedUse {
            shared: shared.clone(),
            id,
        }
    }
}

/// One named use. Dropping it ends the use; the last one to end starts the
/// thing's idle time.
#[must_use = "the use ends as soon as it is dropped"]
pub struct NamedUse {
    shared: Arc<Shared>,
    id: u64,
}
impl NamedUse {
    /// Another named use of the same thing. It needs no turn: the thing
    /// cannot rest while this use is alive, and the new one outlives it if
    /// kept longer.
    pub fn also(&self, name: impl Into<String>) -> NamedUse {
        lock(&self.shared.holders).start(&self.shared, name.into())
    }
}
impl Drop for NamedUse {
    fn drop(&mut self) {
        // Ending a use only ever moves a thing toward rest, so it needs no
        // ordering with opening or resting beyond this short lock.
        let mut holders = lock(&self.shared.holders);
        holders.names.remove(&self.id);
        if holders.names.is_empty() {
            holders.idle_since = Some(Instant::now());
            drop(holders);
            self.shared.freed.notify_waiters();
        }
    }
}
impl std::fmt::Debug for NamedUse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let holders = lock(&self.shared.holders);
        f.debug_struct("NamedUse")
            .field("name", &holders.names.get(&self.id))
            .finish()
    }
}

/// The one turn at a key: while it is held, nothing else opens or rests that
/// thing, and it alone reaches the caller's state, through `Deref`. Uses can
/// still be extended and ended meanwhile.
#[must_use = "the turn ends as soon as it is dropped"]
pub struct UseTurn<S = ()> {
    shared: Arc<Shared>,
    state: OwnedMutexGuard<S>,
}
impl<S> UseTurn<S> {
    /// Whether anything uses the thing now.
    pub fn in_use(&self) -> bool {
        !lock(&self.shared.holders).names.is_empty()
    }
    /// How long nothing has used the thing: `None` while in use, before any
    /// use or wake, and once rested.
    pub fn idle_for(&self) -> Option<Duration> {
        lock(&self.shared.holders).idle_for()
    }
    /// Whether a rest for `idle` is due: nothing uses it, and nothing has
    /// for at least `idle`.
    pub fn due(&self, idle: Duration) -> bool {
        lock(&self.shared.holders).due(idle)
    }
    /// Say the thing may be awake from now, before any use of it is taken:
    /// a wake that fails, panics or is cancelled part way is then still
    /// rested once idle. An idle time already running is kept, so failing
    /// wakes cannot push a rest back.
    pub fn waking(&self) {
        lock(&self.shared.holders)
            .idle_since
            .get_or_insert_with(Instant::now);
    }
    /// Start a named use. Taken under the turn, it is the way a thing's
    /// first use starts.
    pub fn hold(&self, name: impl Into<String>) -> NamedUse {
        lock(&self.shared.holders).start(&self.shared, name.into())
    }
}
impl<S> Deref for UseTurn<S> {
    type Target = S;
    fn deref(&self) -> &S {
        &self.state
    }
}
impl<S> DerefMut for UseTurn<S> {
    fn deref_mut(&mut self) -> &mut S {
        &mut self.state
    }
}
impl<S> std::fmt::Debug for UseTurn<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let holders = lock(&self.shared.holders);
        f.debug_struct("UseTurn")
            .field("uses", &holders.names.values().collect::<Vec<_>>())
            .finish()
    }
}

impl<K: Eq + Hash + Clone, S: Default> Default for Uses<K, S> {
    fn default() -> Self {
        Self {
            entries: SyncMutex::new(HashMap::new()),
        }
    }
}
impl<K, S> std::fmt::Debug for Uses<K, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Uses")
            .field("things", &lock(&self.entries).len())
            .finish()
    }
}
impl<K: Eq + Hash + Clone, S: Default> Uses<K, S> {
    /// No uses yet.
    pub fn new() -> Self {
        Self::default()
    }
    /// Wait for `key`'s turn, to open or rest it.
    pub async fn turn(&self, key: &K) -> UseTurn<S> {
        let (turn, shared) = self.entry(key);
        UseTurn {
            shared,
            state: turn.lock_owned().await,
        }
    }
    /// `key`'s turn if nothing holds it now: a rest pass skips a thing being
    /// opened, since it is about to be used.
    pub fn try_turn(&self, key: &K) -> Option<UseTurn<S>> {
        let (turn, shared) = self.entry(key);
        let state = turn.try_lock_owned().ok()?;
        Some(UseTurn { shared, state })
    }
    /// Say `key`'s thing was stopped, ending `turn`: it is not due for rest
    /// again until it is used or woken, and its entry, with its state, is
    /// forgotten once nothing uses it, holds or waits for its turn.
    pub fn rested(&self, key: &K, turn: UseTurn<S>) {
        {
            let mut holders = lock(&turn.shared.holders);
            if holders.names.is_empty() {
                holders.idle_since = None;
            }
        }
        drop(turn);
        let mut entries = lock(&self.entries);
        let spent = entries.get(key).is_some_and(|entry| {
            // Only the map refers to it: no turn held or awaited, no use.
            Arc::strong_count(&entry.turn) == 1
                && Arc::strong_count(&entry.shared) == 1
                && lock(&entry.shared.holders).idle_since.is_none()
        });
        if spent {
            entries.remove(key);
        }
    }
    /// The names of `key`'s uses now.
    pub fn names(&self, key: &K) -> Vec<String> {
        let Some(shared) = self.shared(key) else {
            return Vec::new();
        };
        let holders = lock(&shared.holders);
        holders.names.values().cloned().collect()
    }
    /// Whether anything uses `key` now.
    pub fn in_use(&self, key: &K) -> bool {
        self.shared(key)
            .is_some_and(|shared| !lock(&shared.holders).names.is_empty())
    }
    /// Answers once no use of `key` is alive: at once if none is now, or
    /// else at the next moment the last one ends. A use taken after that
    /// moment is the caller's to see, with the key's [`UseTurn`].
    pub async fn released(&self, key: &K) {
        let Some(shared) = self.shared(key) else {
            return;
        };
        let freed = shared.freed.notified();
        tokio::pin!(freed);
        // Registered before looking, so an end between the look and the wait
        // is not missed.
        freed.as_mut().enable();
        if lock(&shared.holders).names.is_empty() {
            return;
        }
        freed.await;
    }
    /// Every key nothing has used for at least `idle`, and that has not been
    /// rested since: the ones a rest pass takes the turn of. The map is held
    /// only while its entries are listed.
    pub fn idle(&self, idle: Duration) -> Vec<K> {
        let entries: Vec<_> = lock(&self.entries)
            .iter()
            .map(|(key, entry)| (key.clone(), entry.shared.clone()))
            .collect();
        entries
            .into_iter()
            .filter(|(_, shared)| lock(&shared.holders).due(idle))
            .map(|(key, _)| key)
            .collect()
    }
    /// How many things it keeps an entry for.
    pub fn len(&self) -> usize {
        lock(&self.entries).len()
    }
    /// Whether it keeps no entry.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn shared(&self, key: &K) -> Option<Arc<Shared>> {
        lock(&self.entries)
            .get(key)
            .map(|entry| entry.shared.clone())
    }
    fn entry(&self, key: &K) -> (Arc<Mutex<S>>, Arc<Shared>) {
        let mut entries = lock(&self.entries);
        let entry = entries.entry(key.clone()).or_insert_with(|| {
            Arc::new(Entry {
                turn: Arc::new(Mutex::new(S::default())),
                shared: Arc::new(Shared {
                    holders: SyncMutex::new(Holders::default()),
                    freed: Notify::new(),
                }),
            })
        });
        (entry.turn.clone(), entry.shared.clone())
    }
}

/// Take a short lock. Every change under these locks is a single step, so a
/// panic elsewhere cannot leave them half-changed.
pub(super) fn lock<T>(mutex: &SyncMutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
