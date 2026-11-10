//! Real disposable Git repositories and native managed runs. These establish
//! local command identity and retry behavior, not remote transport qualification.
#![cfg(unix)]
#[path = "support/git_worktree_fixture.rs"]
mod fixture;

use fixture::{assert_once, exited, git, run_case, run_case_format, wait_for, Fixture};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};
use workspace_sdk::{git::GitWorktrees, *};

async fn assert_pin_rejected_before_creation(fixture: &Fixture, anchor: &Workspace, oid: &str) {
    let mut request = fixture.request(anchor, "invalid-pin", "must-not-create");
    request.commit_oid = oid.into();
    request.validate().unwrap(); // The shape is valid; repository semantics are not.
    let (wrapper, counter) = fixture.wrapper("pin-git", false);
    let service = GitWorktrees::new(anchor.clone(), wrapper).unwrap();
    assert!(service.start_create(request.clone()).await.is_err());
    assert!(!Path::new(&request.path).exists());
    assert!(
        !counter.exists(),
        "no Git worktree add invocation is permitted"
    );
    assert!(
        !Path::new(&anchor.descriptor().control_root)
            .join("runs")
            .join(request.run.id.as_str())
            .exists(),
        "invalid pins must not reserve a native run claim"
    );
    assert!(git(
        &fixture.repository,
        &[
            "for-each-ref",
            "--format=%(refname)",
            &request.branch_reference
        ]
    )
    .is_empty());
    assert_eq!(
        fs::read_to_string(fixture.repository.join("shared.txt")).unwrap(),
        "pinned\n"
    );
}

#[tokio::test]
async fn forty_hex_prefix_in_sha256_repository_is_not_a_full_commit_pin() {
    run_case_format("sha256", |fixture| async move {
        assert_eq!(fixture.oid.len(), 64);
        let prefix = &fixture.oid[..40];
        assert_eq!(
            git(
                &fixture.repository,
                &["rev-parse", &format!("{prefix}^{{commit}}")]
            )
            .trim(),
            fixture.oid
        );
        let anchor = fixture.attach(&fixture.repository, "sha256-anchor").await;
        assert_pin_rejected_before_creation(&fixture, &anchor, prefix).await;
    })
    .await;
}

#[tokio::test]
async fn sixty_four_hex_ref_in_sha1_repository_is_not_a_commit_pin() {
    run_case(|fixture| async move {
        assert_eq!(fixture.oid.len(), 40);
        let hex_ref = "a".repeat(64);
        let full_ref = format!("refs/heads/{hex_ref}");
        git(
            &fixture.repository,
            &["update-ref", &full_ref, &fixture.oid],
        );
        assert_eq!(
            git(&fixture.repository, &["rev-parse", &full_ref]).trim(),
            fixture.oid
        );
        let anchor = fixture.attach(&fixture.repository, "sha1-anchor").await;
        assert_pin_rejected_before_creation(&fixture, &anchor, &hex_ref).await;
        assert_eq!(
            git(&fixture.repository, &["rev-parse", &full_ref]).trim(),
            fixture.oid
        );
    })
    .await;
}

#[tokio::test]
async fn annotated_tag_object_cannot_be_silently_peeled_into_a_commit_pin() {
    run_case(|fixture| async move {
        git(
            &fixture.repository,
            &[
                "tag",
                "--annotate",
                "--message",
                "fixture tag",
                "pinned-tag",
                &fixture.oid,
            ],
        );
        let tag_oid = git(&fixture.repository, &["rev-parse", "refs/tags/pinned-tag"])
            .trim()
            .to_owned();
        assert_ne!(tag_oid, fixture.oid);
        assert_eq!(
            git(&fixture.repository, &["cat-file", "-t", &tag_oid]).trim(),
            "tag"
        );
        let anchor = fixture.attach(&fixture.repository, "tag-anchor").await;
        assert_pin_rejected_before_creation(&fixture, &anchor, &tag_oid).await;
    })
    .await;
}

#[tokio::test]
async fn replacement_ref_does_not_change_the_tree_of_a_pinned_commit() {
    run_case(|fixture| async move {
        fs::write(fixture.repository.join("shared.txt"), "replacement tree\n").unwrap();
        git(
            &fixture.repository,
            &["commit", "--quiet", "-am", "replacement commit"],
        );
        let replacement = git(&fixture.repository, &["rev-parse", "HEAD"])
            .trim()
            .to_owned();
        git(
            &fixture.repository,
            &["replace", &fixture.oid, &replacement],
        );
        assert_eq!(
            git(
                &fixture.repository,
                &["show", &format!("{}:shared.txt", fixture.oid)]
            ),
            "replacement tree\n"
        );
        let anchor = fixture
            .attach(&fixture.repository, "replacement-anchor")
            .await;
        let service = GitWorktrees::new(anchor.clone(), "git").unwrap();
        let request = fixture.request(&anchor, "replacement-pin", "pinned-tree");
        let run = service.start_create(request.clone()).await.unwrap();
        assert_eq!(exited(&run).await, RunState::Exited { exit_code: 0 });
        assert_eq!(
            fs::read_to_string(Path::new(&request.path).join("shared.txt")).unwrap(),
            "pinned\n"
        );
        assert_eq!(
            git(Path::new(&request.path), &["rev-parse", "HEAD"]).trim(),
            fixture.oid
        );
        assert_eq!(
            git(
                &fixture.repository,
                &["rev-parse", &format!("refs/replace/{}", fixture.oid)]
            )
            .trim(),
            replacement
        );
        assert_eq!(
            fs::read_to_string(fixture.repository.join("shared.txt")).unwrap(),
            "replacement tree\n"
        );
    })
    .await;
}

#[tokio::test]
async fn pinned_creation_preserves_dirty_anchor_and_supports_discovery_and_explicit_attachment() {
    run_case(|fixture| async move {
        fs::write(fixture.repository.join("shared.txt"), "newer commit\n").unwrap();
        git(
            &fixture.repository,
            &["commit", "--quiet", "-am", "advance main"],
        );
        let newer_oid = git(&fixture.repository, &["rev-parse", "HEAD"]);
        fs::write(fixture.repository.join("shared.txt"), "dirty anchor\n").unwrap();
        fs::write(fixture.repository.join("untracked"), "keep").unwrap();
        let before = git(&fixture.repository, &["status", "--porcelain=v1", "-z"]);
        let anchor = fixture.attach(&fixture.repository, "anchor").await;
        let mut request = fixture.request(&anchor, "create", "created");
        request.path = fixture
            .root
            .join("created space\nline")
            .to_str()
            .unwrap()
            .into();
        let target = PathBuf::from(&request.path);
        let service = GitWorktrees::new(anchor.clone(), "git").unwrap();
        assert_eq!(
            exited(&service.start_create(request).await.unwrap()).await,
            RunState::Exited { exit_code: 0 }
        );
        assert_eq!(git(&target, &["rev-parse", "HEAD"]).trim(), fixture.oid);
        assert_eq!(
            git(&target, &["symbolic-ref", "HEAD"]).trim(),
            "refs/heads/created"
        );
        assert_eq!(
            fs::read_to_string(target.join("shared.txt")).unwrap(),
            "pinned\n"
        );
        assert_eq!(git(&fixture.repository, &["rev-parse", "HEAD"]), newer_oid);
        assert_eq!(
            git(&fixture.repository, &["status", "--porcelain=v1", "-z"]),
            before
        );
        assert_eq!(
            fs::read_to_string(fixture.repository.join("untracked")).unwrap(),
            "keep"
        );
        let inventory = service.discover().await.unwrap();
        assert!(inventory
            .worktrees
            .iter()
            .any(|entry| entry.path == target.to_str().unwrap()));
        assert_eq!(
            fixture.sdk.list().unwrap().len(),
            1,
            "creation does not implicitly register an attachment"
        );
        let attached = fixture.attach(&target, "created-attachment").await;
        assert_eq!(attached.descriptor().ownership, Ownership::Attached);
        let output = attached
            .exec(CommandSpec::new("git").args(["rev-parse", "HEAD"]))
            .await
            .unwrap();
        assert!(output.success());
        assert_eq!(output.stdout.trim(), fixture.oid);
    })
    .await;
}

#[tokio::test]
async fn exact_terminal_retry_reconnects_without_reset_and_rebound_requests_are_refused() {
    run_case(|fixture| async move {
        let anchor = fixture.attach(&fixture.repository, "anchor").await;
        let service = GitWorktrees::new(anchor.clone(), "git").unwrap();
        let request = fixture.request(&anchor, "stable", "created");
        let run = service.start_create(request.clone()).await.unwrap();
        assert_eq!(exited(&run).await, RunState::Exited { exit_code: 0 });
        fs::write(Path::new(&request.path).join("shared.txt"), "dirty target").unwrap();
        let retry = service.start_create(request.clone()).await.unwrap();
        assert_eq!(retry.reference().id, run.reference().id);
        assert_eq!(retry.reference().workspace, run.reference().workspace);
        assert_eq!(exited(&retry).await, RunState::Exited { exit_code: 0 });
        assert_eq!(
            fs::read_to_string(Path::new(&request.path).join("shared.txt")).unwrap(),
            "dirty target"
        );
        let mut changed = request.clone();
        changed.branch_reference = "refs/heads/other".into();
        changed.path = fixture.root.join("other").to_str().unwrap().into();
        assert!(matches!(
            service.start_create(changed).await,
            Err(Error::Conflict(_))
        ));
        assert!(!fixture.root.join("other").exists());
        let other = fixture.attach(&fixture.repository, "other-anchor").await;
        assert!(GitWorktrees::new(other, "git")
            .unwrap()
            .start_create(request)
            .await
            .is_err());
        assert_eq!(
            fs::read_to_string(fixture.root.join("created/shared.txt")).unwrap(),
            "dirty target"
        );
    })
    .await;
}

#[tokio::test]
async fn existing_branch_and_nonempty_destination_fail_without_forced_replacement() {
    run_case(|fixture| async move {
    let anchor = fixture.attach(&fixture.repository, "anchor").await;
    let service = GitWorktrees::new(anchor.clone(), "git").unwrap();
    git(&fixture.repository, &["branch", "occupied", &fixture.oid]);
    let occupied = fixture.request(&anchor, "occupied-run", "occupied");
    assert!(
        matches!(exited(&service.start_create(occupied).await.unwrap()).await, RunState::Exited { exit_code } if exit_code != 0)
    );
    assert_eq!(
        git(&fixture.repository, &["rev-parse", "refs/heads/occupied"]).trim(),
        fixture.oid
    );
    assert!(!fixture.root.join("occupied").exists());
    let destination = fixture.root.join("nonempty");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("keep"), "untouched").unwrap();
    let request = fixture.request(&anchor, "nonempty-run", "nonempty");
    assert!(
        matches!(exited(&service.start_create(request).await.unwrap()).await, RunState::Exited { exit_code } if exit_code != 0)
    );
    assert_eq!(
        fs::read_to_string(destination.join("keep")).unwrap(),
        "untouched"
    );
    assert_eq!(
        git(&fixture.repository, &["symbolic-ref", "HEAD"]).trim(),
        "refs/heads/main"
    );
    }).await;
}

#[tokio::test]
async fn concurrent_identical_create_requests_invoke_git_once() {
    run_case(|fixture| async move {
        let anchor = fixture.attach(&fixture.repository, "anchor").await;
        let (wrapper, counter) = fixture.wrapper("count-git", false);
        let service = GitWorktrees::new(anchor.clone(), wrapper).unwrap();
        let request = fixture.request(&anchor, "concurrent", "created");
        let (first, second) = tokio::join!(
            service.start_create(request.clone()),
            service.start_create(request)
        );
        let first = first.unwrap();
        let second = second.unwrap();
        assert_eq!(first.reference().id, second.reference().id);
        assert_eq!(exited(&first).await, RunState::Exited { exit_code: 0 });
        assert_eq!(exited(&second).await, RunState::Exited { exit_code: 0 });
        assert_once(&counter);
    })
    .await;
}

#[tokio::test]
async fn lost_start_reply_retries_the_native_claim_instead_of_relaunching_git() {
    run_case(|fixture| async move {
    let anchor = fixture.attach(&fixture.repository, "anchor").await;
    let (wrapper, counter) = fixture.wrapper("lost-reply-git", false);
    let service = GitWorktrees::new(anchor.clone(), wrapper).unwrap();
    let request = fixture.request(&anchor, "lost-reply", "created");
    fixture.lost_reply.store(true, Ordering::SeqCst);
    assert!(
        matches!(service.start_create(request.clone()).await, Err(Error::State(message)) if message == "injected lost start reply")
    );
    let retry = service.start_create(request.clone()).await.unwrap();
    assert_eq!(retry.reference().id, request.run.id);
    assert_eq!(exited(&retry).await, RunState::Exited { exit_code: 0 });
    assert_once(&counter);
    assert_eq!(
        git(Path::new(&request.path), &["rev-parse", "HEAD"]).trim(),
        fixture.oid
    );
    }).await;
}

#[tokio::test]
async fn canceled_and_timed_out_create_runs_do_not_relaunch_on_retry() {
    for timed_out in [false, true] {
        run_case(move |fixture| async move {
            let anchor = fixture.attach(&fixture.repository, "anchor").await;
            let (wrapper, counter) = fixture.wrapper("held-git", true);
            let service = GitWorktrees::new(anchor.clone(), wrapper).unwrap();
            let mut request = fixture.request(&anchor, "held", "created");
            request.timeout_seconds = if timed_out { 1 } else { 30 };
            let run = service.start_create(request.clone()).await.unwrap();
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
            let retry = service.start_create(request).await.unwrap();
            assert_eq!(exited(&retry).await, expected);
            assert_once(&counter);
            assert!(!fixture.root.join("created").exists());
        })
        .await;
    }
}

#[tokio::test]
async fn simulated_lost_observation_reuses_the_existing_native_create_claim() {
    // This seam simulates only an uncertain observation. The native supervisor
    // remains alive and owns one real process; actual supervisor-loss behavior
    // is separately qualified by tests/provider_local.rs.
    run_case(|fixture| async move {
        let anchor = fixture.attach(&fixture.repository, "anchor").await;
        let (wrapper, counter) = fixture.wrapper("lost-git", true);
        let service = GitWorktrees::new(anchor.clone(), wrapper).unwrap();
        let request = fixture.request(&anchor, "lost", "created");
        let run = service.start_create(request.clone()).await.unwrap();
        wait_for(&counter).await;
        assert!(run.state().await.unwrap().is_active());
        fixture.simulated_lost.store(true, Ordering::SeqCst);
        assert!(matches!(run.state().await.unwrap(), RunState::Lost { .. }));
        let retry = service.start_create(request).await.unwrap();
        assert_eq!(retry.reference().id, run.reference().id);
        assert!(matches!(
            retry.state().await.unwrap(),
            RunState::Lost { .. }
        ));
        assert_once(&counter);
        assert!(!fixture.root.join("created").exists());
        // Outer cleanup bypasses this observation seam, awaits native cancel,
        // and independently observes its terminal state before dropping storage.
    })
    .await;
}
