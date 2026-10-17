//! Real Git discovery and existing LocalProvider attachments. Explicit Git
//! worktree creation remains a caller command, not an SDK lifecycle guarantee.
#![cfg(unix)]
mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};
use tempfile::TempDir;
use workspace_sdk::{
    git::{GitWorktree, GitWorktreeDiscovery, GitWorktreeHead},
    infrastructure::FileStateRepository,
    providers::{LocalConfig, LocalOpenOptions, LocalProvider},
    CommandSpec, Error, OpenRequest, Ownership, ProviderId, RunState, Workspace, WorkspaceId,
    WorkspaceSdk,
};

fn git(root: &Path, arguments: &[&str]) -> String {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|key| key.starts_with("GIT_")) {
            command.env_remove(key);
        }
    }
    let output = command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-C",
        ])
        .arg(root)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fixture Git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn repository(parent: &Path, name: &str) -> PathBuf {
    let root = parent.join(name);
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "--quiet", "--initial-branch=main"]);
    // Explicit SDK-executed Git commands must also stay inside the disposable
    // fixture's hook/signing configuration, independent of host preferences.
    git(&root, &["config", "--local", "core.hooksPath", "/dev/null"]);
    git(&root, &["config", "--local", "commit.gpgSign", "false"]);
    fs::write(root.join("counter"), "0").unwrap();
    fs::write(root.join("shared.txt"), "baseline").unwrap();
    git(&root, &["add", "counter", "shared.txt"]);
    git(
        &root,
        &["commit", "--quiet", "-m", "fixture initial commit"],
    );
    root
}
fn sdk(root: &Path) -> WorkspaceSdk {
    let registry = Arc::new(FileStateRepository::new(root.join("registry")).unwrap());
    let mut config = LocalConfig::trusted_host(root.join("owned"));
    config.supervisor = env!("CARGO_BIN_EXE_workspace-supervisor").into();
    WorkspaceSdk::builder(registry)
        .provider(Arc::new(LocalProvider::new(config).unwrap()))
        .unwrap()
        .build()
        .unwrap()
}
async fn attach(sdk: &WorkspaceSdk, path: &Path, id: &str) -> Workspace {
    sdk.open(
        &ProviderId::new("local").unwrap(),
        OpenRequest::new(
            WorkspaceId::new(id).unwrap(),
            &LocalOpenOptions {
                attach: Some(path.to_owned()),
            },
        )
        .unwrap(),
    )
    .await
    .unwrap()
}
fn entry<'a>(entries: &'a [GitWorktree], path: &Path) -> &'a GitWorktree {
    entries
        .iter()
        .find(|item| item.path == path.to_str().unwrap())
        .expect("Git must report the expected host-local path")
}
fn assert_branch(entry: &GitWorktree, branch: &str, oid: &str) {
    match &entry.head {
        GitWorktreeHead::Branch {
            reference,
            oid: actual,
        } => {
            assert_eq!(reference, &format!("refs/heads/{branch}"));
            assert_eq!(actual, oid);
        }
        head => panic!("expected branch head, got {head:?}"),
    }
}

#[tokio::test]
async fn real_git_inventory_preserves_linked_paths_and_metadata_from_either_anchor() {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let repository = repository(&root, "main repository");
    let spaced = root.join("linked worktree with spaces");
    let newline = root.join("linked\nworktree");
    let locked = root.join("locked without reason");
    let missing = root.join("missing worktree");
    git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "linked",
            spaced.to_str().unwrap(),
        ],
    );
    git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            newline.to_str().unwrap(),
        ],
    );
    git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "locked",
            locked.to_str().unwrap(),
        ],
    );
    git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "missing",
            missing.to_str().unwrap(),
        ],
    );
    git(
        &repository,
        &[
            "worktree",
            "lock",
            "--reason",
            "keep this linked checkout",
            spaced.to_str().unwrap(),
        ],
    );
    git(&repository, &["worktree", "lock", locked.to_str().unwrap()]);
    fs::remove_dir_all(&missing).unwrap();
    let oid = git(&repository, &["rev-parse", "HEAD"]).trim().to_owned();
    let sdk = sdk(&root.join("controller"));
    let primary_anchor = attach(&sdk, &repository, "primary-anchor").await;
    let linked_anchor = attach(&sdk, &spaced, "linked-anchor").await;
    for anchor in [primary_anchor, linked_anchor] {
        let expected_anchor = anchor.reference();
        let inventory = GitWorktreeDiscovery::new(anchor, "git")
            .unwrap()
            .discover()
            .await
            .unwrap();
        assert_eq!(inventory.anchor, expected_anchor);
        assert_eq!(inventory.worktrees.len(), 5);
        assert_branch(entry(&inventory.worktrees, &repository), "main", &oid);
        let linked = entry(&inventory.worktrees, &spaced);
        assert_branch(linked, "linked", &oid);
        assert_eq!(linked.locked.as_deref(), Some("keep this linked checkout"));
        assert!(linked.prunable.is_none());
        match &entry(&inventory.worktrees, &newline).head {
            GitWorktreeHead::Detached { oid: actual } => assert_eq!(actual, &oid),
            head => panic!("expected detached head, got {head:?}"),
        }
        assert_eq!(
            entry(&inventory.worktrees, &locked).locked.as_deref(),
            Some("")
        );
        assert!(entry(&inventory.worktrees, &missing)
            .prunable
            .as_ref()
            .is_some_and(|reason| !reason.is_empty()));
    }
    assert!(
        !missing.exists(),
        "discovery must not repair or recreate a prunable checkout"
    );
    assert_eq!(git(&repository, &["rev-parse", "HEAD"]).trim(), oid);
}

#[tokio::test]
async fn bare_and_unborn_git_repositories_are_explicit_observations() {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let bare = root.join("bare repository");
    let unborn = root.join("unborn repository");
    fs::create_dir(&bare).unwrap();
    fs::create_dir(&unborn).unwrap();
    git(&bare, &["init", "--quiet", "--bare"]);
    git(&unborn, &["init", "--quiet", "--initial-branch=main"]);
    let sdk = sdk(&root.join("controller"));
    let bare_anchor = attach(&sdk, &bare, "bare-anchor").await;
    let bare_inventory = GitWorktreeDiscovery::new(bare_anchor, "git")
        .unwrap()
        .discover()
        .await
        .unwrap();
    assert_eq!(bare_inventory.worktrees.len(), 1);
    assert!(matches!(
        entry(&bare_inventory.worktrees, &bare).head,
        GitWorktreeHead::Bare
    ));
    let unborn_anchor = attach(&sdk, &unborn, "unborn-anchor").await;
    let inventory = GitWorktreeDiscovery::new(unborn_anchor, "git")
        .unwrap()
        .discover()
        .await
        .unwrap();
    assert_eq!(inventory.worktrees.len(), 1);
    match &entry(&inventory.worktrees, &unborn).head {
        GitWorktreeHead::Branch { reference, oid } => {
            assert_eq!(reference, "refs/heads/main");
            assert!(matches!(oid.len(), 40 | 64));
            assert!(oid.bytes().all(|byte| byte == b'0'));
        }
        head => panic!("expected an unborn branch, got {head:?}"),
    }
}

#[tokio::test]
async fn discovered_existing_and_explicitly_created_worktrees_execute_independently() {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let repository = repository(&root, "main repository");
    let preexisting = root.join("preexisting worktree");
    let created = root.join("created through anchor\nworktree");
    git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "preexisting",
            preexisting.to_str().unwrap(),
        ],
    );
    let sdk = sdk(&root.join("controller"));
    let anchor = attach(&sdk, &repository, "execution-anchor").await;
    // The caller deliberately issues this Git command. This does not grant the
    // SDK ownership of the Git worktree or promise idempotent Git creation.
    let creation = anchor
        .exec(
            CommandSpec::new("git")
                .args([
                    "--no-pager",
                    "worktree",
                    "add",
                    "--quiet",
                    "-b",
                    "created",
                    created.to_str().unwrap(),
                ])
                .timeout(10),
        )
        .await
        .unwrap();
    assert!(creation.success());
    assert!(!creation.output_truncated);
    let inventory = GitWorktreeDiscovery::new(anchor.clone(), "git")
        .unwrap()
        .discover()
        .await
        .unwrap();
    assert_eq!(inventory.anchor, anchor.reference());
    assert_eq!(inventory.worktrees.len(), 3);
    let first_path = PathBuf::from(&entry(&inventory.worktrees, &preexisting).path);
    let second_path = PathBuf::from(&entry(&inventory.worktrees, &created).path);
    let first = attach(&sdk, &first_path, "first-execution").await;
    let second = attach(&sdk, &second_path, "second-execution").await;
    let first_descriptor = first.descriptor();
    let second_descriptor = second.descriptor();
    assert_eq!(first_descriptor.ownership, Ownership::Attached);
    assert_eq!(second_descriptor.ownership, Ownership::Attached);
    assert_ne!(
        first_descriptor.control_root,
        second_descriptor.control_root
    );
    for descriptor in [&first_descriptor, &second_descriptor] {
        assert!(!Path::new(&descriptor.control_root).starts_with(&repository));
        assert!(!Path::new(&descriptor.control_root).starts_with(&preexisting));
        assert!(!Path::new(&descriptor.control_root).starts_with(&created));
    }
    let (foreground, managed) = tokio::join!(
        first.exec(CommandSpec::new(support::workload_program()).args([
            "increment",
            "counter",
            "1"
        ])),
        second.start(CommandSpec::new(support::workload_program()).args([
            "increment",
            "counter",
            "2"
        ]))
    );
    assert!(foreground.unwrap().success());
    let managed = managed.unwrap();
    assert_eq!(managed.reference().workspace, second.reference());
    assert_eq!(
        managed.wait(Duration::from_secs(10)).await.unwrap(),
        RunState::Exited { exit_code: 0 }
    );
    assert_eq!(
        fs::read_to_string(preexisting.join("counter")).unwrap(),
        "1"
    );
    assert_eq!(fs::read_to_string(created.join("counter")).unwrap(), "2");
    assert_eq!(fs::read_to_string(repository.join("counter")).unwrap(), "0");
    for path in [&repository, &preexisting, &created] {
        assert_eq!(
            fs::read_to_string(path.join("shared.txt")).unwrap(),
            "baseline"
        );
    }
    assert!(!first_descriptor.capabilities.destroy && !second_descriptor.capabilities.destroy);
    assert!(matches!(first.destroy().await, Err(Error::Unsupported(_))));
    assert!(matches!(second.destroy().await, Err(Error::Unsupported(_))));
    assert!(repository.is_dir() && preexisting.is_dir() && created.is_dir());
    assert_eq!(
        sdk.list().unwrap().len(),
        3,
        "only explicit attachments enter the existing SDK registry"
    );
}

#[tokio::test]
async fn discovery_does_not_convert_git_failure_into_an_empty_inventory() {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let project = root.join("ordinary directory");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("keep"), "untouched").unwrap();
    let sdk = sdk(&root.join("controller"));
    let anchor = attach(&sdk, &project, "not-a-repository").await;
    let discovery = GitWorktreeDiscovery::new(anchor.clone(), "git").unwrap();
    assert!(discovery.discover().await.is_err());
    let unavailable = root.join("missing git executable");
    let discovery = GitWorktreeDiscovery::new(anchor, unavailable.to_str().unwrap()).unwrap();
    assert!(discovery.discover().await.is_err());
    assert_eq!(
        fs::read_to_string(project.join("keep")).unwrap(),
        "untouched"
    );
    assert!(!project.join(".git").exists());
    assert_eq!(sdk.list().unwrap().len(), 1);
}
