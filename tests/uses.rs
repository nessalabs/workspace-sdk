//! Named uses of a caller's own things, and the turn that orders opening
//! against resting.
use std::time::Duration;
use workspace_sdk::Uses;

const IDLE: Duration = Duration::from_secs(300);

#[tokio::test(start_paused = true)]
async fn a_thing_is_in_use_exactly_while_a_named_use_is_alive() {
    let uses = Uses::new();
    assert!(!uses.in_use(&"vm"));
    assert!(
        uses.idle(Duration::ZERO).is_empty(),
        "never used, never due"
    );
    let lease = uses.hold(&"vm", "lease");
    let command = uses.hold(&"vm", "command");
    assert!(uses.in_use(&"vm"));
    let mut names = uses.uses(&"vm");
    names.sort();
    assert_eq!(names, ["command", "lease"]);
    drop(lease);
    assert_eq!(uses.uses(&"vm"), ["command"]);
    assert!(uses.idle(Duration::ZERO).is_empty(), "still in use");
    drop(command);
    assert!(!uses.in_use(&"vm"));
    assert_eq!(uses.idle(Duration::ZERO), ["vm"]);
    assert!(uses.idle(IDLE).is_empty(), "idle time has not passed");
    tokio::time::advance(IDLE).await;
    assert_eq!(uses.idle(IDLE), ["vm"]);
}

#[tokio::test(start_paused = true)]
async fn a_rest_is_due_only_after_the_idle_time_and_not_again_once_rested() {
    let uses = Uses::new();
    drop(uses.hold(&"vm", "lease"));
    let turn = uses.turn(&"vm").await;
    assert!(!turn.due(IDLE));
    tokio::time::advance(IDLE).await;
    assert!(turn.due(IDLE));
    turn.rested();
    assert!(uses.idle(Duration::ZERO).is_empty());
    assert!(!uses.turn(&"vm").await.due(Duration::ZERO));
    // Used again, it is due again once that use ends.
    drop(uses.hold(&"vm", "lease"));
    assert_eq!(uses.idle(Duration::ZERO), ["vm"]);
}

#[tokio::test(start_paused = true)]
async fn one_turn_at_a_time_while_uses_come_and_go_without_waiting() {
    let uses = Uses::new();
    let opening = uses.turn(&"vm").await;
    assert!(uses.try_turn(&"vm").is_none(), "an opening holds the turn");
    assert!(uses.try_turn(&"other").is_some(), "turns are per key");
    // A use is taken during another's turn, without waiting for it.
    let lease = uses.hold(&"vm", "lease");
    assert!(opening.in_use());
    let rest = tokio::spawn({
        let uses = std::sync::Arc::new(uses);
        let waiting = uses.clone();
        async move {
            let turn = waiting.turn(&"vm").await;
            (turn.in_use(), turn.due(Duration::ZERO))
        }
    });
    tokio::task::yield_now().await;
    assert!(!rest.is_finished(), "the rest waits for the opening's turn");
    drop(opening);
    assert_eq!(rest.await.unwrap(), (true, false), "and then sees the use");
    drop(lease);
}

#[tokio::test(start_paused = true)]
async fn a_wake_that_never_became_a_use_is_still_rested_and_cannot_put_it_off() {
    let uses = Uses::new();
    let turn = uses.turn(&"vm").await;
    turn.waking();
    drop(turn);
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
async fn a_rested_turn_keeps_a_use_taken_meanwhile() {
    let uses = Uses::new();
    drop(uses.hold(&"vm", "lease"));
    let turn = uses.turn(&"vm").await;
    let late = uses.hold(&"vm", "late");
    turn.rested();
    assert!(uses.in_use(&"vm"));
    drop(late);
    assert_eq!(
        uses.idle(Duration::ZERO),
        ["vm"],
        "its end starts the idle time"
    );
}

#[tokio::test(start_paused = true)]
async fn released_answers_when_the_last_use_ends() {
    let uses = std::sync::Arc::new(Uses::new());
    uses.released(&"vm").await;
    let lease = uses.hold(&"vm", "lease");
    let command = uses.hold(&"vm", "command");
    let waiting = tokio::spawn({
        let uses = uses.clone();
        async move { uses.released(&"vm").await }
    });
    tokio::task::yield_now().await;
    drop(lease);
    tokio::task::yield_now().await;
    assert!(!waiting.is_finished(), "a use is still alive");
    drop(command);
    tokio::time::timeout(Duration::from_secs(1), waiting)
        .await
        .expect("answered once the last use ended")
        .unwrap();
}
