//! A machine stays awake while a named use holds it, and the one rest path
//! stops it only after the caller's idle time with no use. The local provider's
//! stop ends its owned runs, which is what these tests observe.
use std::{sync::Arc, time::Duration};
use tempfile::TempDir;
use workspace_sdk::{
    infrastructure::FileStateRepository,
    providers::{LocalConfig, LocalProvider},
    *,
};

const IDLE: Duration = Duration::from_millis(500);

async fn setup(temp: &TempDir) -> (Arc<Machines>, WorkspaceId) {
    let mut config = LocalConfig::trusted_host(temp.path().join("targets"));
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    let repository = Arc::new(FileStateRepository::new(temp.path().join("state")).unwrap());
    let sdk = WorkspaceSdk::builder(repository)
        .provider(Arc::new(LocalProvider::new(config).unwrap()))
        .unwrap()
        .build()
        .unwrap();
    let id = WorkspaceId::new("rests").unwrap();
    sdk.create(
        &ProviderId::new("local").unwrap(),
        CreateRequest::new(id.clone()),
    )
    .await
    .unwrap();
    (Arc::new(Machines::new(Arc::new(sdk), IDLE)), id)
}

fn sleeper(id: &str) -> LaunchRequest {
    LaunchRequest {
        id: RunId::new(id).unwrap(),
        execution: CommandSpec::new("sleep").args(["30"]).timeout(60).into(),
        output_limit_bytes: 1024,
    }
}

#[tokio::test]
async fn a_held_machine_never_rests_and_an_idle_one_rests_once() {
    let temp = TempDir::new().unwrap();
    let (machines, id) = setup(&temp).await;
    let first = machines.hold(&id, "agent").await.unwrap();
    let second = machines.hold(&id, "terminal").await.unwrap();
    let run = first.start_with(sleeper("long")).await.unwrap();
    assert_eq!(machines.uses(&id), ["agent", "terminal"]);

    drop(first);
    tokio::time::sleep(IDLE * 2).await;
    assert!(machines.rest_idle().await.rested.is_empty());
    assert!(!run.state().await.unwrap().is_terminal());

    drop(second);
    assert!(machines.uses(&id).is_empty());
    // Not idle long enough yet.
    assert!(machines.rest_idle().await.rested.is_empty());
    tokio::time::sleep(IDLE * 2).await;
    let report = machines.rest_idle().await;
    assert_eq!(report.rested, std::slice::from_ref(&id));
    assert!(report.failed.is_empty());
    assert!(run.state().await.unwrap().is_terminal());
    // Already at rest: nothing to stop again.
    assert!(machines.rest_idle().await.rested.is_empty());
}

#[tokio::test]
async fn a_new_use_wakes_a_rested_machine_and_restarts_the_idle_clock() {
    let temp = TempDir::new().unwrap();
    let (machines, id) = setup(&temp).await;
    drop(machines.hold(&id, "first").await.unwrap());
    tokio::time::sleep(IDLE * 2).await;
    assert_eq!(machines.rest_idle().await.rested, std::slice::from_ref(&id));

    let again = machines.hold(&id, "second").await.unwrap();
    let run = again.start_with(sleeper("after-wake")).await.unwrap();
    assert!(!run.state().await.unwrap().is_terminal());
    drop(again);
    // The clock starts at the end of the latest use, not the first.
    assert!(machines.rest_idle().await.rested.is_empty());
    assert!(!run.state().await.unwrap().is_terminal());
    // Leave nothing running.
    run.cancel().await.unwrap();
}

#[tokio::test]
async fn background_rest_reports_what_it_stopped() {
    let temp = TempDir::new().unwrap();
    let (machines, id) = setup(&temp).await;
    drop(machines.hold(&id, "brief").await.unwrap());
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let task = machines.rest_every(Duration::from_millis(50), move |report| {
        let _ = sender.send(report.rested);
    });
    let rested = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    drop(task);
    assert_eq!(rested, [id]);
}

#[tokio::test]
async fn an_attached_machine_cannot_be_held_for_rest() {
    let temp = TempDir::new().unwrap();
    let mut config = LocalConfig::trusted_host(temp.path().join("targets"));
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    let repository = Arc::new(FileStateRepository::new(temp.path().join("state")).unwrap());
    let sdk = WorkspaceSdk::builder(repository)
        .provider(Arc::new(LocalProvider::new(config).unwrap()))
        .unwrap()
        .build()
        .unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let id = WorkspaceId::new("attached").unwrap();
    sdk.attach(
        &ProviderId::new("local").unwrap(),
        AttachRequest::new(id.clone(), &providers::LocalAttachOptions { path: project }).unwrap(),
    )
    .await
    .unwrap();
    let machines = Machines::new(Arc::new(sdk), IDLE);
    assert!(matches!(
        machines.hold(&id, "agent").await,
        Err(Error::Conflict(_))
    ));
    assert!(machines.uses(&id).is_empty());
}

#[tokio::test]
async fn a_use_opened_during_a_rest_pass_keeps_its_machine_awake() {
    let temp = TempDir::new().unwrap();
    let (machines, id) = setup(&temp).await;
    drop(machines.hold(&id, "brief").await.unwrap());
    tokio::time::sleep(IDLE * 2).await;
    // Race a new use against the rest pass many times: whichever runs first,
    // the use must end up on an awake machine.
    for round in 0..5 {
        let (held, _) = tokio::join!(
            machines.hold(&id, format!("racer-{round}")),
            machines.rest_idle()
        );
        let held = held.unwrap();
        let run = held
            .start_with(sleeper(&format!("race-{round}")))
            .await
            .unwrap();
        assert!(!run.state().await.unwrap().is_terminal());
        assert!(machines.rest_idle().await.rested.is_empty());
        run.cancel().await.unwrap();
        drop(held);
        tokio::time::sleep(IDLE * 2).await;
        assert_eq!(machines.rest_idle().await.rested, std::slice::from_ref(&id));
    }
}
