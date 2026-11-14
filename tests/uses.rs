//! Named uses of a caller's own things, and the turn that orders opening
//! against resting.
use std::{future::Future, sync::Arc, task::Poll, time::Duration};
use workspace_sdk::Uses;

const IDLE: Duration = Duration::from_secs(300);

#[tokio::test(start_paused = true)]
async fn a_thing_is_in_use_exactly_while_a_named_use_is_alive() {
    let uses: Uses<&str> = Uses::new();
    assert!(!uses.in_use(&"vm"));
    assert!(
        uses.idle(Duration::ZERO).is_empty(),
        "never used, never due"
    );
    let lease = uses.turn(&"vm").await.hold("lease");
    let command = lease.also("command");
    assert!(uses.in_use(&"vm"));
    let mut names = uses.names(&"vm");
    names.sort();
    assert_eq!(names, ["command", "lease"]);
    drop(lease);
    assert_eq!(
        uses.names(&"vm"),
        ["command"],
        "an extended use outlives the first"
    );
    assert!(uses.idle(Duration::ZERO).is_empty(), "still in use");
    drop(command);
    assert!(!uses.in_use(&"vm"));
    assert_eq!(uses.idle(Duration::ZERO), ["vm"]);
    assert!(uses.idle(IDLE).is_empty(), "idle time has not passed");
    tokio::time::advance(IDLE).await;
    assert_eq!(uses.idle(IDLE), ["vm"]);
}

#[tokio::test(start_paused = true)]
async fn a_rest_is_due_only_after_the_idle_time_and_its_entry_is_then_forgotten() {
    let uses: Uses<&str> = Uses::new();
    drop(uses.turn(&"vm").await.hold("lease"));
    let turn = uses.turn(&"vm").await;
    assert!(!turn.due(IDLE));
    tokio::time::advance(IDLE).await;
    assert!(turn.due(IDLE));
    uses.rested(&"vm", turn);
    assert!(uses.idle(Duration::ZERO).is_empty());
    assert_eq!(uses.len(), 0, "nothing refers to it any more");
    // Used again, it is due again once that use ends.
    drop(uses.turn(&"vm").await.hold("lease"));
    assert_eq!(uses.idle(Duration::ZERO), ["vm"]);
}

#[tokio::test(start_paused = true)]
async fn a_rested_thing_is_kept_while_a_use_or_a_turn_still_refers_to_it() {
    let uses: Arc<Uses<&str, u32>> = Arc::new(Uses::new());
    let lease = uses.turn(&"vm").await.hold("lease");
    let mut turn = uses.turn(&"vm").await;
    *turn = 7;
    // A use extended meanwhile keeps it in use, and kept.
    let late = lease.also("late");
    drop(lease);
    uses.rested(&"vm", turn);
    assert!(uses.in_use(&"vm"));
    assert_eq!(uses.len(), 1);
    drop(late);
    assert_eq!(
        uses.idle(Duration::ZERO),
        ["vm"],
        "its end starts the idle time"
    );
    // A turn awaited during a rest keeps the entry and its state.
    let resting = uses.turn(&"vm").await;
    let waiting = tokio::spawn({
        let uses = uses.clone();
        async move { *uses.turn(&"vm").await }
    });
    tokio::task::yield_now().await;
    uses.rested(&"vm", resting);
    assert_eq!(uses.len(), 1, "a turn is still awaited");
    assert_eq!(waiting.await.unwrap(), 7, "the state it kept");
}

#[tokio::test(start_paused = true)]
async fn one_turn_at_a_time_while_uses_come_and_go_without_waiting() {
    let uses: Arc<Uses<&str>> = Arc::new(Uses::new());
    let opening = uses.turn(&"vm").await;
    assert!(uses.try_turn(&"vm").is_none(), "an opening holds the turn");
    assert!(uses.try_turn(&"other").is_some(), "turns are per key");
    let lease = opening.hold("lease");
    let command = lease.also("command");
    let rest = tokio::spawn({
        let uses = uses.clone();
        async move {
            let turn = uses.turn(&"vm").await;
            (turn.in_use(), turn.due(Duration::ZERO))
        }
    });
    tokio::task::yield_now().await;
    assert!(!rest.is_finished(), "the rest waits for the opening's turn");
    drop(opening);
    assert_eq!(rest.await.unwrap(), (true, false), "and then sees the uses");
    drop((lease, command));
}

#[tokio::test(start_paused = true)]
async fn a_use_between_listing_and_turn_keeps_the_thing_from_rest() {
    let uses: Uses<&str> = Uses::new();
    drop(uses.turn(&"vm").await.hold("lease"));
    tokio::time::advance(IDLE).await;
    assert_eq!(uses.idle(IDLE), ["vm"]);
    // A use taken after the list was made, under its own turn.
    let late = uses.turn(&"vm").await.hold("late");
    let turn = uses.try_turn(&"vm").unwrap();
    assert!(!turn.due(IDLE));
    drop(late);
}

#[tokio::test(start_paused = true)]
async fn a_wake_that_never_became_a_use_is_still_rested_and_cannot_put_it_off() {
    let uses: Uses<&str> = Uses::new();
    uses.turn(&"vm").await.waking();
    tokio::time::advance(IDLE).await;
    // A second failing wake keeps the time the first one started.
    uses.turn(&"vm").await.waking();
    assert_eq!(uses.idle(IDLE), ["vm"]);
    let turn = uses.turn(&"vm").await;
    assert!(turn.due(IDLE));
    // A use taken after a wake ends its idle time.
    let lease = turn.hold("lease");
    assert_eq!(turn.idle_for(), None);
    drop(lease);
    assert_eq!(turn.idle_for(), Some(Duration::ZERO));
}

#[tokio::test(start_paused = true)]
async fn released_answers_when_the_last_use_ends_even_between_look_and_wait() {
    let uses: Uses<&str> = Uses::new();
    uses.released(&"vm").await;
    assert_eq!(uses.len(), 0, "a read keeps nothing");
    let lease = uses.turn(&"vm").await.hold("lease");
    let command = lease.also("command");
    let mut released = std::pin::pin!(uses.released(&"vm"));
    // Looked once, while in use: it waits.
    std::future::poll_fn(|context| {
        assert!(released.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(lease);
    std::future::poll_fn(|context| {
        assert!(
            released.as_mut().poll(context).is_pending(),
            "a use is alive"
        );
        Poll::Ready(())
    })
    .await;
    // The last use ends before it is polled again.
    drop(command);
    tokio::time::timeout(Duration::from_secs(1), released)
        .await
        .expect("answered once the last use ended");
}
