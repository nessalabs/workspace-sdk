//! Explicit non-force Git removal on disposable native workspaces. Ignored files
//! may be deleted by Git; refusal is not a transactional rollback guarantee.
#![cfg(unix)]
#[path = "support/git_worktree_fixture.rs"]
mod fixture;

use fixture::{assert_once, exited, git, run_case, wait_for, Fixture};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};
use workspace_sdk::{
    git::{GitWorktreeRemoveRequest, GitWorktrees},
    *,
};

fn request(anchor: &Workspace, id: &str, path: &Path) -> GitWorktreeRemoveRequest {
    GitWorktreeRemoveRequest::new(
        RunRef {
            workspace: anchor.reference(),
            id: RunId::new(id).unwrap(),
        },
        path.to_str().unwrap(),
    )
    .unwrap()
}

async fn linked(fixture: &Fixture, anchor: &Workspace, branch: &str) -> PathBuf {
    let mut creation = fixture.request(anchor, &format!("create-{branch}"), branch);
    creation.commit_oid = git(&fixture.repository, &["rev-parse", "HEAD"])
        .trim()
        .into();
    let path = PathBuf::from(&creation.path);
    let service = GitWorktrees::new(anchor.clone(), "git").unwrap();
    assert_eq!(
        exited(&service.start_create(creation).await.unwrap()).await,
        RunState::Exited { exit_code: 0 }
    );
    path
}

fn assert_failed(state: RunState) {
    assert!(matches!(state, RunState::Exited { exit_code } if exit_code != 0));
}

#[tokio::test]
async fn clean_removal_accepts_literal_space_newline_path_and_keeps_the_branch() {
    run_case(|fixture| async move {
        let anchor = fixture.attach(&fixture.repository, "anchor").await;
        let mut creation = fixture.request(&anchor, "create", "linked");
        creation.path = fixture
            .root
            .join("linked space\nline")
            .to_str()
            .unwrap()
            .into();
        let path = PathBuf::from(&creation.path);
        let (create_wrapper, create_counter) = fixture.wrapper("create-git", false);
        assert_eq!(
            exited(
                &GitWorktrees::new(anchor.clone(), create_wrapper)
                    .unwrap()
                    .start_create(creation)
                    .await
                    .unwrap()
            )
            .await,
            RunState::Exited { exit_code: 0 }
        );
        assert_once(&create_counter);
        let service = GitWorktrees::new(anchor.clone(), "git").unwrap();
        let removal = service
            .start_remove(request(&anchor, "remove", &path))
            .await
            .unwrap();
        assert_eq!(exited(&removal).await, RunState::Exited { exit_code: 0 });
        assert!(!path.exists());
        assert_eq!(
            git(&fixture.repository, &["rev-parse", "refs/heads/linked"]).trim(),
            fixture.oid
        );
        let inventory = service.discover().await.unwrap();
        assert_eq!(inventory.worktrees.len(), 1);
        assert_eq!(
            inventory.worktrees[0].path,
            fixture.repository.to_str().unwrap()
        );
        assert_eq!(
            fs::read_to_string(fixture.repository.join("shared.txt")).unwrap(),
            "pinned\n"
        );
        assert_eq!(fixture.sdk.list().unwrap().len(), 1);
    })
    .await;
}

#[tokio::test]
async fn exact_retry_after_path_reuse_reconnects_without_removing_the_replacement() {
    run_case(|fixture| async move {
        let anchor = fixture.attach(&fixture.repository, "anchor").await;
        let path = linked(&fixture, &anchor, "original").await;
        let (wrapper, counter) = fixture.wrapper_for("remove-git", false, "remove");
        let service = GitWorktrees::new(anchor.clone(), wrapper).unwrap();
        let original_request = request(&anchor, "stable-remove", &path);
        let run = service
            .start_remove(original_request.clone())
            .await
            .unwrap();
        assert_eq!(exited(&run).await, RunState::Exited { exit_code: 0 });
        git(
            &fixture.repository,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "replacement",
                path.to_str().unwrap(),
                &fixture.oid,
            ],
        );
        let retry = service.start_remove(original_request).await.unwrap();
        assert_eq!(retry.reference().id, run.reference().id);
        assert_eq!(retry.reference().workspace, run.reference().workspace);
        assert_eq!(exited(&retry).await, RunState::Exited { exit_code: 0 });
        assert_once(&counter);
        assert_eq!(
            git(&path, &["symbolic-ref", "HEAD"]).trim(),
            "refs/heads/replacement"
        );
        assert_eq!(
            fs::read_to_string(path.join("shared.txt")).unwrap(),
            "pinned\n"
        );
    })
    .await;
}

#[tokio::test]
async fn rebound_path_timeout_output_and_wrong_workspace_are_refused() {
    run_case(|fixture| async move {
        let anchor = fixture.attach(&fixture.repository, "anchor").await;
        let removed = linked(&fixture, &anchor, "removed").await;
        let surviving = linked(&fixture, &anchor, "surviving").await;
        let service = GitWorktrees::new(anchor.clone(), "git").unwrap();
        let original = request(&anchor, "bound", &removed);
        assert_eq!(
            exited(&service.start_remove(original.clone()).await.unwrap()).await,
            RunState::Exited { exit_code: 0 }
        );
        let mut changed_path = original.clone();
        changed_path.path = surviving.to_str().unwrap().into();
        let mut changed_timeout = original.clone();
        changed_timeout.timeout_seconds += 1;
        let mut changed_output = original.clone();
        changed_output.output_limit_bytes += 1;
        for changed in [changed_path, changed_timeout, changed_output] {
            assert!(matches!(
                service.start_remove(changed).await,
                Err(Error::Conflict(_))
            ));
        }
        // Creation and removal share the native identity namespace. A creation
        // receipt must never become permission to remove its resulting checkout.
        assert!(matches!(
            service
                .start_remove(request(&anchor, "create-surviving", &surviving))
                .await,
            Err(Error::Conflict(_))
        ));
        let other = fixture.attach(&fixture.repository, "other-anchor").await;
        assert!(GitWorktrees::new(other, "git")
            .unwrap()
            .start_remove(original)
            .await
            .is_err());
        assert_eq!(
            fs::read_to_string(surviving.join("shared.txt")).unwrap(),
            "pinned\n"
        );
    })
    .await;
}

#[tokio::test]
async fn tracked_and_ordinary_untracked_files_refuse_nonforce_removal() {
    run_case(|fixture| async move {
        let anchor = fixture.attach(&fixture.repository, "anchor").await;
        let tracked = linked(&fixture, &anchor, "tracked").await;
        let untracked = linked(&fixture, &anchor, "untracked").await;
        fs::write(tracked.join("shared.txt"), "user edit").unwrap();
        fs::write(untracked.join("untracked.txt"), "user data").unwrap();
        // This host/repository setting must not hide ordinary untracked data
        // from the non-force command's explicit status override.
        git(
            &fixture.repository,
            &["config", "--local", "status.showUntrackedFiles", "no"],
        );
        let (wrapper, counter) = fixture.wrapper_for("dirty-remove", false, "remove");
        let service = GitWorktrees::new(anchor.clone(), wrapper).unwrap();
        let original = request(&anchor, "tracked-remove", &tracked);
        let first = service.start_remove(original.clone()).await.unwrap();
        let failed = exited(&first).await;
        assert_failed(failed.clone());
        assert_eq!(
            fs::read_to_string(tracked.join("shared.txt")).unwrap(),
            "user edit"
        );
        // Cleaning the target does not authorize a previously failed request
        // to execute again: this exact run must reconnect to its old failure.
        git(&tracked, &["restore", "--worktree", "shared.txt"]);
        let retry = service.start_remove(original).await.unwrap();
        assert_eq!(retry.reference().id, first.reference().id);
        assert_eq!(exited(&retry).await, failed);
        assert_once(&counter);
        assert_eq!(
            fs::read_to_string(tracked.join("shared.txt")).unwrap(),
            "pinned\n"
        );
        let plain = GitWorktrees::new(anchor.clone(), "git").unwrap();
        assert_failed(
            exited(
                &plain
                    .start_remove(request(&anchor, "untracked-remove", &untracked))
                    .await
                    .unwrap(),
            )
            .await,
        );
        assert_eq!(
            fs::read_to_string(untracked.join("untracked.txt")).unwrap(),
            "user data"
        );
        assert!(
            git(&fixture.repository, &["worktree", "list", "--porcelain"])
                .contains("refs/heads/untracked")
        );
    })
    .await;
}

#[tokio::test]
async fn locked_main_and_unregistered_destinations_are_not_forced() {
    run_case(|fixture| async move {
        let anchor = fixture.attach(&fixture.repository, "anchor").await;
        let locked = linked(&fixture, &anchor, "locked").await;
        git(
            &fixture.repository,
            &[
                "worktree",
                "lock",
                "--reason",
                "fixture lock",
                locked.to_str().unwrap(),
            ],
        );
        let unrelated = fixture.root.join("unregistered");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("keep"), "untouched").unwrap();
        let service = GitWorktrees::new(anchor.clone(), "git").unwrap();
        for (id, path) in [
            ("locked-remove", &locked),
            ("unregistered-remove", &unrelated),
        ] {
            assert_failed(
                exited(
                    &service
                        .start_remove(request(&anchor, id, path))
                        .await
                        .unwrap(),
                )
                .await,
            );
        }
        // Use a surviving linked anchor so Git's own main-worktree guard, not
        // the SDK's same-anchor guard, receives this request.
        let linked_anchor = fixture.attach(&locked, "linked-anchor").await;
        let main_remove = GitWorktrees::new(linked_anchor.clone(), "git")
            .unwrap()
            .start_remove(request(&linked_anchor, "main-remove", &fixture.repository))
            .await
            .unwrap();
        assert_failed(exited(&main_remove).await);
        assert!(fixture.repository.join(".git").is_dir());
        assert_eq!(
            fs::read_to_string(unrelated.join("keep")).unwrap(),
            "untouched"
        );
        let inventory = service.discover().await.unwrap();
        assert!(inventory
            .worktrees
            .iter()
            .any(|entry| entry.path == locked.to_str().unwrap()
                && entry.locked.as_deref() == Some("fixture lock")));
    })
    .await;
}

#[tokio::test]
async fn ignored_files_are_explicitly_deleted_by_successful_git_removal() {
    run_case(|fixture| async move {
        fs::write(fixture.repository.join(".gitignore"), "ignored.bin\n").unwrap();
        git(&fixture.repository, &["add", ".gitignore"]);
        git(
            &fixture.repository,
            &["commit", "--quiet", "-m", "ignore fixture data"],
        );
        let anchor = fixture.attach(&fixture.repository, "anchor").await;
        let path = linked(&fixture, &anchor, "ignored").await;
        fs::write(path.join("ignored.bin"), b"ignored disposable data").unwrap();
        let service = GitWorktrees::new(anchor.clone(), "git").unwrap();
        assert_eq!(
            exited(
                &service
                    .start_remove(request(&anchor, "remove-ignored", &path))
                    .await
                    .unwrap()
            )
            .await,
            RunState::Exited { exit_code: 0 }
        );
        assert!(
            !path.exists(),
            "non-force Git still deletes ignored data; caller authorization covers this"
        );
        assert!(fixture.repository.join(".gitignore").is_file());
    })
    .await;
}

#[tokio::test]
async fn anchor_ancestor_and_control_subtree_are_refused_before_launch() {
    run_case(|fixture| async move {
        let anchor = fixture.attach(&fixture.repository, "anchor").await;
        let target = linked(&fixture, &anchor, "control-containing").await;
        let (wrapper, counter) = fixture.wrapper_for("guard-git", false, "remove");
        let service = GitWorktrees::new(anchor.clone(), wrapper).unwrap();
        for (id, path) in [("anchor", &fixture.repository), ("ancestor", &fixture.root)] {
            assert!(service
                .start_remove(request(&anchor, id, path))
                .await
                .is_err());
            assert!(!Path::new(&anchor.descriptor().control_root)
                .join("runs")
                .join(id)
                .exists());
        }
        let original_descriptor = anchor.descriptor();
        let mut descriptor = original_descriptor.clone();
        descriptor.control_root = target.join("private-control").to_str().unwrap().into();
        *fixture.descriptor_override.lock().unwrap() = Some(descriptor);
        assert!(service
            .start_remove(request(&anchor, "control", &target))
            .await
            .is_err());
        *fixture.descriptor_override.lock().unwrap() = None;
        assert!(!Path::new(&original_descriptor.control_root)
            .join("runs/control")
            .exists());
        assert!(
            !counter.exists(),
            "pure guards must run before managed Git launch"
        );
        assert_eq!(
            fs::read_to_string(target.join("shared.txt")).unwrap(),
            "pinned\n"
        );
        assert!(fixture.repository.join(".git").is_dir());
    })
    .await;
}

#[tokio::test]
async fn concurrent_identical_removals_invoke_git_once() {
    run_case(|fixture| async move {
        let anchor = fixture.attach(&fixture.repository, "anchor").await;
        let path = linked(&fixture, &anchor, "concurrent").await;
        let (wrapper, counter) = fixture.wrapper_for("concurrent-remove", false, "remove");
        let service = GitWorktrees::new(anchor.clone(), wrapper).unwrap();
        let removal = request(&anchor, "remove", &path);
        let (first, second) = tokio::join!(
            service.start_remove(removal.clone()),
            service.start_remove(removal)
        );
        let first = first.unwrap();
        let second = second.unwrap();
        assert_eq!(first.reference().id, second.reference().id);
        assert_eq!(exited(&first).await, RunState::Exited { exit_code: 0 });
        assert_eq!(exited(&second).await, RunState::Exited { exit_code: 0 });
        assert_once(&counter);
        assert!(!path.exists());
    })
    .await;
}

#[tokio::test]
async fn lost_start_ack_retries_removal_claim_without_replaying_git() {
    run_case(|fixture| async move {
        let anchor = fixture.attach(&fixture.repository, "anchor").await;
        let path = linked(&fixture, &anchor, "lost-ack").await;
        let (wrapper, counter) = fixture.wrapper_for("lost-ack-remove", false, "remove");
        let service = GitWorktrees::new(anchor.clone(), wrapper).unwrap();
        let removal = request(&anchor, "remove", &path);
        fixture.lost_reply.store(true, Ordering::SeqCst);
        assert!(matches!(service.start_remove(removal.clone()).await, Err(Error::State(message)) if message == "injected lost start reply"));
        let retry = service.start_remove(removal).await.unwrap();
        assert_eq!(exited(&retry).await, RunState::Exited { exit_code: 0 });
        assert_once(&counter);
        assert!(!path.exists());
    }).await;
}

#[tokio::test]
async fn canceled_and_timed_out_pre_git_removals_do_not_relaunch() {
    for timed_out in [false, true] {
        run_case(move |fixture| async move {
            let anchor = fixture.attach(&fixture.repository, "anchor").await;
            let path = linked(&fixture, &anchor, "held").await;
            let (wrapper, counter) = fixture.wrapper_for("held-remove", true, "remove");
            let service = GitWorktrees::new(anchor.clone(), wrapper).unwrap();
            let mut removal = request(&anchor, "remove", &path);
            removal.timeout_seconds = if timed_out { 1 } else { 30 };
            let run = service.start_remove(removal.clone()).await.unwrap();
            wait_for(&counter).await;
            if !timed_out {
                run.cancel().await.unwrap();
            }
            let expected = if timed_out {
                RunState::TimedOut
            } else {
                RunState::Canceled
            };
            assert_eq!(exited(&run).await, expected);
            assert_eq!(
                exited(&service.start_remove(removal).await.unwrap()).await,
                expected
            );
            assert_once(&counter);
            assert_eq!(
                fs::read_to_string(path.join("shared.txt")).unwrap(),
                "pinned\n"
            );
            // This wrapper never entered Git. These checks do not claim that
            // cancellation during a real Git remove rolls back partial deletion.
        })
        .await;
    }
}

#[tokio::test]
async fn simulated_lost_observation_reuses_the_native_remove_claim() {
    run_case(|fixture| async move {
        let anchor = fixture.attach(&fixture.repository, "anchor").await;
        let path = linked(&fixture, &anchor, "lost").await;
        let (wrapper, counter) = fixture.wrapper_for("lost-remove", true, "remove");
        let service = GitWorktrees::new(anchor.clone(), wrapper).unwrap();
        let removal = request(&anchor, "remove", &path);
        let run = service.start_remove(removal.clone()).await.unwrap();
        wait_for(&counter).await;
        assert!(run.state().await.unwrap().is_active());
        fixture.simulated_lost.store(true, Ordering::SeqCst);
        assert!(matches!(run.state().await.unwrap(), RunState::Lost { .. }));
        let retry = service.start_remove(removal).await.unwrap();
        assert!(matches!(
            retry.state().await.unwrap(),
            RunState::Lost { .. }
        ));
        assert_once(&counter);
        assert!(path.join("shared.txt").is_file());
        // The real native supervisor is alive. Outer cleanup bypasses this
        // simulated observation and confirms native cancellation and termination.
    })
    .await;
}

#[tokio::test]
async fn populated_submodule_refuses_nonforce_removal() {
    run_case(|fixture| async move {
        let submodule = fixture.root.join("submodule-source");
        fs::create_dir(&submodule).unwrap();
        git(&submodule, &["init", "--quiet", "--initial-branch=main"]);
        git(
            &submodule,
            &["config", "--local", "core.hooksPath", "/dev/null"],
        );
        git(
            &submodule,
            &["config", "--local", "commit.gpgSign", "false"],
        );
        fs::write(submodule.join("data"), "submodule content").unwrap();
        git(&submodule, &["add", "data"]);
        git(
            &submodule,
            &["commit", "--quiet", "-m", "submodule fixture"],
        );
        git(
            &fixture.repository,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "--quiet",
                submodule.to_str().unwrap(),
                "nested-module",
            ],
        );
        git(
            &fixture.repository,
            &["commit", "--quiet", "-am", "add submodule"],
        );
        let anchor = fixture.attach(&fixture.repository, "anchor").await;
        let path = linked(&fixture, &anchor, "submodule-linked").await;
        git(
            &path,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "update",
                "--init",
                "--quiet",
            ],
        );
        assert_eq!(
            fs::read_to_string(path.join("nested-module/data")).unwrap(),
            "submodule content"
        );
        let service = GitWorktrees::new(anchor.clone(), "git").unwrap();
        assert_failed(
            exited(
                &service
                    .start_remove(request(&anchor, "remove", &path))
                    .await
                    .unwrap(),
            )
            .await,
        );
        assert_eq!(
            fs::read_to_string(path.join("nested-module/data")).unwrap(),
            "submodule content"
        );
        assert!(path.join("shared.txt").is_file());
    })
    .await;
}
