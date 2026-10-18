//! Deterministic status/lock ordering on real disposable files and OS locks.
//! These force a possible interleaving, not the cause of any particular CI failure.
//! Explicit fixture unlock releases flock ownership across descriptors inherited
//! by unrelated concurrent test children. Native production release is unchanged.
use super::{filesystem, inspect, inspect_after_status};
use crate::RunState;
use std::fs;

#[test]
fn terminal_publication_between_status_read_and_lock_check_is_not_lost() {
    for initial in [RunState::Starting, RunState::Running] {
        for terminal in [
            RunState::Exited { exit_code: 0 },
            RunState::Exited { exit_code: 127 },
            RunState::Canceled,
            RunState::TimedOut,
        ] {
            let temporary = tempfile::tempdir().unwrap();
            let path = temporary.path();
            let owner = filesystem::lock(&path.join("active.lock")).unwrap();
            filesystem::write_json(&path.join("status.json"), &initial).unwrap();
            // Capture exactly the observation that production inspect makes
            // before testing liveness, while the supervisor still owns its lock.
            let observed: RunState = filesystem::read_json(&path.join("status.json")).unwrap();
            assert_eq!(observed, initial);
            // Normal supervisor ordering: publish the stopped result first,
            // then release liveness. No sleep or probabilistic thread race.
            filesystem::write_json(&path.join("status.json"), &terminal).unwrap();
            let published = fs::read(path.join("status.json")).unwrap();
            fs2::FileExt::unlock(&owner).unwrap();
            drop(owner);
            assert_eq!(inspect_after_status(path, observed).unwrap(), terminal);
            assert_eq!(inspect(path).unwrap(), terminal);
            assert_eq!(fs::read(path.join("status.json")).unwrap(), published);
        }
    }
}

#[test]
fn held_native_liveness_lock_keeps_active_observations_active() {
    for active in [RunState::Starting, RunState::Running] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path();
        let owner = filesystem::lock(&path.join("active.lock")).unwrap();
        filesystem::write_json(&path.join("status.json"), &active).unwrap();
        let observed = filesystem::read_json(&path.join("status.json")).unwrap();
        assert_eq!(inspect_after_status(path, observed).unwrap(), active);
        assert_eq!(inspect(path).unwrap(), active);
        fs2::FileExt::unlock(&owner).unwrap();
        drop(owner);
    }
}

#[test]
fn released_lock_with_still_active_status_remains_unknown_not_terminal() {
    for active in [RunState::Starting, RunState::Running] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path();
        let owner = filesystem::lock(&path.join("active.lock")).unwrap();
        filesystem::write_json(&path.join("status.json"), &active).unwrap();
        let observed: RunState = filesystem::read_json(&path.join("status.json")).unwrap();
        assert_eq!(observed, active);
        let original = fs::read(path.join("status.json")).unwrap();
        fs2::FileExt::unlock(&owner).unwrap();
        // Simulate disappeared liveness without a stopped receipt.
        drop(owner);
        // Consume one real liveness observation. A second query could observe
        // a probe descriptor inherited by an unrelated concurrent fork.
        let after = inspect(path).unwrap();
        assert!(matches!(after, RunState::Lost { .. }));
        assert!(!after.is_terminal());
        assert_eq!(fs::read(path.join("status.json")).unwrap(), original);
    }
}

#[test]
fn already_observed_terminal_and_lost_states_do_not_require_a_liveness_lock() {
    for state in [
        RunState::Exited { exit_code: 0 },
        RunState::Exited { exit_code: 127 },
        RunState::Canceled,
        RunState::TimedOut,
        RunState::Lost {
            reason: "recorded uncertainty must remain visible".into(),
        },
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path();
        filesystem::write_json(&path.join("status.json"), &state).unwrap();
        // An invalid lock entry makes an unintended lock access fail explicitly.
        fs::create_dir(path.join("active.lock")).unwrap();
        let observed = filesystem::read_json(&path.join("status.json")).unwrap();
        assert_eq!(inspect_after_status(path, observed).unwrap(), state);
        assert_eq!(inspect(path).unwrap(), state);
    }
}

#[test]
fn freshly_published_lost_reason_is_preserved_without_claiming_terminal_completion() {
    for initial in [RunState::Starting, RunState::Running] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path();
        let owner = filesystem::lock(&path.join("active.lock")).unwrap();
        filesystem::write_json(&path.join("status.json"), &initial).unwrap();
        let observed = filesystem::read_json(&path.join("status.json")).unwrap();
        let lost = RunState::Lost {
            reason: "persisted observation failure after accepted launch".into(),
        };
        filesystem::write_json(&path.join("status.json"), &lost).unwrap();
        let published = fs::read(path.join("status.json")).unwrap();
        fs2::FileExt::unlock(&owner).unwrap();
        drop(owner);
        let result = inspect_after_status(path, observed).unwrap();
        assert_eq!(result, lost);
        assert!(!result.is_terminal());
        assert_eq!(inspect(path).unwrap(), lost);
        assert_eq!(fs::read(path.join("status.json")).unwrap(), published);
    }
}

#[test]
fn missing_or_malformed_status_after_liveness_release_is_an_error() {
    for missing in [true, false] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path();
        let owner = filesystem::lock(&path.join("active.lock")).unwrap();
        filesystem::write_json(&path.join("status.json"), &RunState::Starting).unwrap();
        let observed = filesystem::read_json(&path.join("status.json")).unwrap();
        if missing {
            fs::remove_file(path.join("status.json")).unwrap();
        } else {
            fs::write(path.join("status.json"), b"invalid status JSON").unwrap();
        }
        fs2::FileExt::unlock(&owner).unwrap();
        drop(owner);
        assert!(inspect_after_status(path, observed).is_err());
        assert!(inspect(path).is_err());
        if missing {
            assert!(!path.join("status.json").exists());
        } else {
            assert_eq!(
                fs::read(path.join("status.json")).unwrap(),
                b"invalid status JSON"
            );
        }
    }
}
