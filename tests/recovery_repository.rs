//! Shared authority contracts at real memory and fsync-backed repository boundaries.
use std::sync::{Arc, Barrier};
use workspace_sdk::{
    checkpoint::CheckpointRef,
    domain::{
        HandoffPhase, HandoffReceipt, OperationId, ProviderId, ProviderOptions, RecoveryCandidate,
        RecoveryHead, RecoverySession, ResourceRequirements, RunId, RunRef, SyncSession,
        WorkflowRecord, WorkspaceId, WorkspaceLocator, WorkspaceRef,
    },
    infrastructure::{FileStateRepository, MemoryStateRepository},
    ports::{IRecoveryRepository, IWorkflowRepository},
    Error,
};

fn seed() -> RecoverySession {
    RecoverySession {
        id: OperationId::new("recovery").unwrap(),
        identity: "recipe".into(),
        candidates: vec![RecoveryCandidate {
            provider: ProviderId::new("laptop").unwrap(),
            config: ProviderOptions::default(),
            resources: ResourceRequirements::default(),
        }],
        revision: 0,
        committed: RecoveryHead {
            generation: 0,
            checkpoint: CheckpointRef("seed".into()),
        },
        current_attempt: None,
    }
}
fn sync(id: OperationId) -> SyncSession {
    SyncSession {
        id,
        workspace: WorkspaceRef {
            version: 1,
            id: WorkspaceId::new("workspace").unwrap(),
            provider: ProviderId::new("laptop").unwrap(),
            locator: WorkspaceLocator::from_locator(&"fixture").unwrap(),
        },
        local_root: "/project".into(),
        baseline: CheckpointRef("seed".into()),
        latest_reconciliation: None,
        pending_remote: None,
        initialized: true,
        last_apply: None,
    }
}
fn contract(repository: &(impl IRecoveryRepository + IWorkflowRepository)) {
    let initial = seed();
    assert!(repository.load_recovery(&initial.id).unwrap().is_none());
    repository
        .compare_exchange_recovery(&initial.id, None, &initial)
        .unwrap();
    assert!(matches!(
        repository.compare_exchange_recovery(&initial.id, None, &initial),
        Err(Error::Conflict(_))
    ));
    let mut next = initial.clone();
    next.revision = 1;
    repository
        .compare_exchange_recovery(&initial.id, Some(0), &next)
        .unwrap();
    assert!(matches!(
        repository.compare_exchange_recovery(&initial.id, Some(0), &next),
        Err(Error::Conflict(_))
    ));
    let mut invalid = next.clone();
    invalid.revision = 2;
    invalid.identity = "rebound".into();
    assert!(repository
        .compare_exchange_recovery(&initial.id, Some(1), &invalid)
        .is_err());
    assert!(repository
        .compare_exchange_recovery(&OperationId::new("wrong").unwrap(), None, &initial)
        .is_err());
    assert_eq!(
        repository
            .load_recovery(&initial.id)
            .unwrap()
            .unwrap()
            .revision,
        1
    );
    assert!(matches!(
        IWorkflowRepository::save(repository, &initial.id, &WorkflowRecord::Recovery(next)),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        IWorkflowRepository::save(repository, &initial.id, &sync(initial.id.clone()).into()),
        Err(Error::Conflict(_))
    ));
    let other = sync(OperationId::new("sync").unwrap());
    IWorkflowRepository::save(repository, &other.id, &other.clone().into()).unwrap();
    assert!(matches!(
        repository.load_recovery(&other.id),
        Err(Error::Conflict(_))
    ));
    let mut collision = initial.clone();
    collision.id = other.id.clone();
    assert!(matches!(
        repository.compare_exchange_recovery(&other.id, None, &collision),
        Err(Error::Conflict(_))
    ));
    let absent = seed();
    let mut bypass = absent.clone();
    bypass.id = OperationId::new("bypass").unwrap();
    assert!(IWorkflowRepository::save(repository, &bypass.id, &bypass.clone().into()).is_err());
    assert!(repository.load_recovery(&bypass.id).unwrap().is_none());
    let lock = repository.acquire_recovery(&initial.id).unwrap();
    assert!(matches!(
        repository.acquire_recovery(&initial.id),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        IWorkflowRepository::acquire(repository, &initial.id),
        Err(Error::Conflict(_))
    ));
    let other_lock = repository
        .acquire_recovery(&OperationId::new("unrelated").unwrap())
        .unwrap();
    drop(other_lock);
    drop(lock);
    assert!(repository.acquire_recovery(&initial.id).is_ok());
}
#[test]
fn memory_obeys_authority_contract() {
    contract(&MemoryStateRepository::default());
}
#[test]
fn file_obeys_authority_contract_and_survives_reopen() {
    let directory = tempfile::tempdir().unwrap();
    contract(&FileStateRepository::new(directory.path()).unwrap());
    let reopened = FileStateRepository::new(directory.path()).unwrap();
    assert_eq!(
        reopened
            .load_recovery(&seed().id)
            .unwrap()
            .unwrap()
            .revision,
        1
    );
}
fn race(first: Arc<dyn IRecoveryRepository>, second: Arc<dyn IRecoveryRepository>) {
    let initial = seed();
    first
        .compare_exchange_recovery(&initial.id, None, &initial)
        .unwrap();
    let start = Arc::new(Barrier::new(3));
    let handles = [first.clone(), second].map(|repository| {
        let start = start.clone();
        let mut next = initial.clone();
        next.revision = 1;
        std::thread::spawn(move || {
            start.wait();
            repository.compare_exchange_recovery(&next.id, Some(0), &next)
        })
    });
    start.wait();
    let results = handles.map(|handle| handle.join().unwrap());
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(Error::Conflict(_))))
            .count(),
        1
    );
    assert_eq!(
        first.load_recovery(&initial.id).unwrap().unwrap().revision,
        1
    );
}
#[test]
fn memory_compare_exchange_has_one_winner() {
    let repository = Arc::new(MemoryStateRepository::default());
    race(repository.clone(), repository);
}
#[test]
fn separate_file_controllers_compare_exchange_has_one_winner() {
    let directory = tempfile::tempdir().unwrap();
    race(
        Arc::new(FileStateRepository::new(directory.path()).unwrap()),
        Arc::new(FileStateRepository::new(directory.path()).unwrap()),
    );
}
#[test]
fn old_typed_state_migrates_and_invalid_authority_is_not_discarded() {
    let directory = tempfile::tempdir().unwrap();
    let original = sync(OperationId::new("legacy-sync").unwrap());
    let handoff = HandoffReceipt {
        id: OperationId::new("legacy-handoff").unwrap(),
        identity: "inputs".into(),
        source: RunRef {
            workspace: original.workspace.clone(),
            id: RunId::new("stopped").unwrap(),
        },
        destination: original.workspace.clone(),
        checkpoint: original.baseline.clone(),
        phase: HandoffPhase::Restored,
        destination_run: None,
    };
    let path = directory.path().join("state.json");
    for version in [1, 2] {
        let legacy = if version == 1 {
            serde_json::to_value(&original).unwrap()
        } else {
            serde_json::to_value(WorkflowRecord::Sync(original.clone())).unwrap()
        };
        let legacy_handoff = if version == 1 {
            serde_json::to_value(&handoff).unwrap()
        } else {
            serde_json::to_value(WorkflowRecord::Handoff(handoff.clone())).unwrap()
        };
        std::fs::write(&path, serde_json::to_vec(&serde_json::json!({"version":version,"reservations":{},"workspaces":{},"workflows":{"legacy-sync":legacy,"legacy-handoff":legacy_handoff}})).unwrap()).unwrap();
        let repository = FileStateRepository::new(directory.path()).unwrap();
        assert!(
            IWorkflowRepository::load(&repository, &original.id)
                .unwrap()
                .unwrap()
                .into_sync()
                .unwrap()
                .initialized
        );
        let loaded_handoff = IWorkflowRepository::load(&repository, &handoff.id)
            .unwrap()
            .unwrap()
            .into_handoff()
            .unwrap();
        assert!(matches!(loaded_handoff.phase, HandoffPhase::Restored));
        assert_eq!(loaded_handoff.source.id, handoff.source.id);
        let recovery = seed();
        repository
            .compare_exchange_recovery(&recovery.id, None, &recovery)
            .unwrap();
        let stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored["version"], 3);
        assert!(stored["workflows"]["legacy-sync"].is_object());
    }
    let mut corrupted = seed();
    corrupted.candidates.clear();
    std::fs::write(&path, serde_json::to_vec(&serde_json::json!({"version":3,"reservations":{},"workspaces":{},"workflows":{"recovery":WorkflowRecord::Recovery(corrupted)}})).unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(FileStateRepository::new(directory.path())
        .unwrap()
        .load_recovery(&seed().id)
        .is_err());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}
#[test]
fn file_effect_lock_is_shared_with_other_controller_and_released_on_owner_drop() {
    let directory = tempfile::tempdir().unwrap();
    let first = FileStateRepository::new(directory.path()).unwrap();
    let second = FileStateRepository::new(directory.path()).unwrap();
    let id = seed().id;
    let lease = first.acquire_recovery(&id).unwrap();
    assert!(matches!(
        second.acquire_recovery(&id),
        Err(Error::Conflict(_))
    ));
    drop(lease);
    assert!(second.acquire_recovery(&id).is_ok());
}

#[test]
fn rejected_file_cas_does_not_rewrite_authority_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let repository = FileStateRepository::new(directory.path()).unwrap();
    let initial = seed();
    repository
        .compare_exchange_recovery(&initial.id, None, &initial)
        .unwrap();
    let path = directory.path().join("state.json");
    let durable = std::fs::read(&path).unwrap();
    let mut invalid = initial.clone();
    invalid.revision = 1;
    invalid.identity = "different-config".into();
    assert!(repository
        .compare_exchange_recovery(&initial.id, Some(0), &invalid)
        .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), durable);
    assert!(repository
        .compare_exchange_recovery(&initial.id, Some(999), &initial)
        .is_err());
    assert_eq!(std::fs::read(path).unwrap(), durable);
}
