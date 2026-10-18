//! Full archive validation without publishing a proof-only Git checkpoint.
use base64::Engine;
use sha2::Digest;
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex,
    },
    time::Instant,
};
use workspace_sdk::{checkpoint::*, Error, Result};

struct CountingGit {
    executable: PathBuf,
    calls: PathBuf,
    disabled: PathBuf,
}
impl CountingGit {
    fn new(root: &Path) -> Self {
        let calls = root.join("git-calls");
        let disabled = root.join("git-disabled");
        let executable = root.join("counted-git");
        let script=format!("#!/usr/bin/python3\nimport os,sys\nwith open({},'ab') as log: log.write(b'git\\n')\nif os.path.exists({}): sys.exit(97)\nos.execvp('git',['git']+sys.argv[1:])\n",serde_json::to_string(calls.to_str().unwrap()).unwrap(),serde_json::to_string(disabled.to_str().unwrap()).unwrap());
        fs::write(&executable, script).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            executable,
            calls,
            disabled,
        }
    }
    fn count(&self) -> usize {
        fs::read(&self.calls)
            .map(|bytes| bytes.iter().filter(|byte| **byte == b'\n').count())
            .unwrap_or(0)
    }
}
fn git(root: &Path, args: &[&str]) {
    let result = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
fn artifacts(state: &Path) -> usize {
    fs::read_dir(state.join("checkpoints")).unwrap().count()
}
fn state_bytes(root: &Path) -> u64 {
    let mut pending = vec![root.to_path_buf()];
    let mut size = 0;
    let mut entries = 0;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            entries += 1;
            assert!(entries <= 2048);
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                assert!(metadata.is_file());
                size += metadata.len();
            }
        }
    }
    size
}
fn source(root: &Path) -> PathBuf {
    let source = root.join("project");
    fs::create_dir(&source).unwrap();
    git(&source, &["init", "--quiet"]);
    fs::write(source.join(".gitignore"), "*.ignore\n").unwrap();
    fs::write(source.join("tracked.ignore"), "selected tracked bytes").unwrap();
    git(&source, &["add", "-f", "tracked.ignore"]);
    fs::write(source.join("untracked.ignore"), "excluded").unwrap();
    fs::write(source.join(".env"), "excluded synthetic secret").unwrap();
    let mut word = 0x1234_5678u32;
    let bytes: Vec<_> = (0..2 * 1024 * 1024)
        .map(|_| {
            word ^= word << 13;
            word ^= word >> 17;
            word ^= word << 5;
            word as u8
        })
        .collect();
    fs::write(source.join("binary"), bytes).unwrap();
    fs::write(source.join("small"), "first-small-edit").unwrap();
    fs::write(source.join("executable"), "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        fs::set_permissions(source.join("executable"), fs::Permissions::from_mode(0o755)).unwrap();
        symlink("binary", source.join("link")).unwrap();
    }
    source
}

#[test]
fn validated_fingerprint_matches_capture_without_git_or_artifact_publication() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let wrapper = CountingGit::new(root);
    let state = root.join("state");
    let engine = GitCheckpointEngine::new(&wrapper.executable, &state).unwrap();
    let project = source(root);
    let selected = engine.export_tree_archive(&project).unwrap();
    let initial = engine
        .capture_archive(&selected, CaptureOptions::default())
        .unwrap();
    let names: Vec<_> = initial
        .manifest
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    assert!(names.contains(&"tracked.ignore"));
    assert!(!names.contains(&"untracked.ignore"));
    assert!(!names.contains(&".env"));
    #[cfg(unix)]
    {
        assert_eq!(
            initial
                .manifest
                .entries
                .iter()
                .find(|entry| entry.path == "executable")
                .unwrap()
                .mode,
            100755
        );
        assert_eq!(
            initial
                .manifest
                .entries
                .iter()
                .find(|entry| entry.path == "link")
                .unwrap()
                .kind,
            EntryKind::Symlink
        );
    }
    let before = (artifacts(&state), state_bytes(&state), wrapper.count());
    fs::write(&wrapper.disabled, b"reject any Git process").unwrap();
    assert_eq!(
        engine.fingerprint_archive(&selected).unwrap(),
        initial.manifest.tree_fingerprint
    );
    assert_eq!(
        (artifacts(&state), state_bytes(&state), wrapper.count()),
        before
    );
    assert!(
        engine
            .capture_archive(&selected, CaptureOptions::default())
            .is_err(),
        "control proves Git is genuinely unavailable"
    );
    assert!(wrapper.count() > before.2);
    fs::remove_file(&wrapper.disabled).unwrap();
    let mut observations = Vec::new();
    for edit in ["second-small-edit", "third-small-edit"] {
        fs::write(project.join("small"), edit).unwrap();
        let archive = engine.export_tree_archive(&project).unwrap();
        let before = (artifacts(&state), state_bytes(&state), wrapper.count());
        let started = Instant::now();
        let checkpoint = engine
            .capture_archive(&archive, CaptureOptions::default())
            .unwrap();
        let capture_ms = started.elapsed().as_secs_f64() * 1000.0;
        let after_capture = (artifacts(&state), state_bytes(&state), wrapper.count());
        let started = Instant::now();
        let fingerprint = engine.fingerprint_archive(&archive).unwrap();
        let fingerprint_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(fingerprint, checkpoint.manifest.tree_fingerprint);
        assert_eq!(
            (artifacts(&state), state_bytes(&state), wrapper.count()),
            after_capture
        );
        assert_eq!(after_capture.0, before.0 + 1);
        observations.push(serde_json::json!({"archive_bytes":archive.len(),"capture_ms":capture_ms,"fingerprint_ms":fingerprint_ms,"capture_artifacts_added":after_capture.0-before.0,"capture_state_bytes_added":after_capture.1-before.1,"capture_git_calls":after_capture.2-before.2,"fingerprint_artifacts_added":0,"fingerprint_state_bytes_added":0,"fingerprint_git_calls":0}));
    }
    println!(
        "{}",
        serde_json::json!({"profile":"same-archive-capture-vs-fingerprint","large_file_bytes":2*1024*1024,"observations":observations,"baseline":false,"bandwidth_or_memory_claim":false})
    );
}

#[test]
fn malformed_archive_fingerprints_fail_complete_validation_without_artifacts() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let wrapper = CountingGit::new(root);
    let state = root.join("state");
    let engine = GitCheckpointEngine::new(&wrapper.executable, &state).unwrap();
    let project = root.join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("file"), b"binary\0\xff").unwrap();
    let valid: serde_json::Value =
        serde_json::from_slice(&engine.export_tree_archive(&project).unwrap()).unwrap();
    let before = (artifacts(&state), state_bytes(&state), wrapper.count());
    let mut cases = vec![b"{malformed".to_vec()];
    for mutation in 0..9 {
        let mut archive = valid.clone();
        match mutation {
            0 => archive["entries"][0]["sha256"] = serde_json::json!("0".repeat(64)),
            1 => archive["entries"][0]["size"] = serde_json::json!(0),
            2 => archive["entries"][0]["path"] = serde_json::json!("../escape"),
            3 => {
                let entry = archive["entries"][0].clone();
                archive["entries"].as_array_mut().unwrap().push(entry);
            }
            4 => archive["entries"][0]["data_base64"] = serde_json::json!("not-base64"),
            5 => archive["entries"][0]["mode"] = serde_json::json!(100777),
            6 => archive["entries"][0]["path"] = serde_json::json!(".env"),
            7 => archive["version"] = serde_json::json!(999),
            _ => {
                let target = b"../escape";
                archive["entries"][0]["kind"] = serde_json::json!("symlink");
                archive["entries"][0]["mode"] = serde_json::json!(120000);
                archive["entries"][0]["size"] = serde_json::json!(target.len());
                archive["entries"][0]["sha256"] =
                    serde_json::json!(format!("{:x}", sha2::Sha256::digest(target)));
                archive["entries"][0]["data_base64"] =
                    serde_json::json!(base64::engine::general_purpose::STANDARD.encode(target));
            }
        }
        cases.push(serde_json::to_vec(&archive).unwrap());
    }
    for bytes in cases {
        assert!(engine.fingerprint_archive(&bytes).is_err());
        assert_eq!(
            (artifacts(&state), state_bytes(&state), wrapper.count()),
            before
        );
    }
    let limited = engine
        .with_limits(CheckpointLimits {
            max_file_bytes: 1,
            ..CheckpointLimits::default()
        })
        .unwrap();
    assert!(limited
        .fingerprint_archive(&serde_json::to_vec(&valid).unwrap())
        .is_err());
    assert_eq!(
        (artifacts(&state), state_bytes(&state), wrapper.count()),
        before
    );
    let encoded_limit = GitCheckpointEngine::new(&wrapper.executable, root.join("tiny-state"))
        .unwrap()
        .with_limits(CheckpointLimits {
            max_total_bytes: 1,
            max_manifest_bytes: 1,
            ..CheckpointLimits::default()
        })
        .unwrap();
    assert!(encoded_limit
        .fingerprint_archive(&serde_json::to_vec(&valid).unwrap())
        .is_err());
    assert_eq!(artifacts(&root.join("tiny-state")), 0);
}

struct LegacyEngine {
    inner: GitCheckpointEngine,
    calls: AtomicUsize,
    fail: AtomicBool,
    observed: Mutex<Option<(Vec<u8>, CaptureOptions)>>,
}
impl ICheckpointEngine for LegacyEngine {
    fn capture(&self, root: &Path, options: CaptureOptions) -> Result<Checkpoint> {
        self.inner.capture(root, options)
    }
    fn export(&self, id: &CheckpointRef) -> Result<CheckpointBundle> {
        self.inner.export(id)
    }
    fn import(&self, bundle: &CheckpointBundle) -> Result<Checkpoint> {
        self.inner.import(bundle)
    }
    fn restore(&self, id: &CheckpointRef, destination: &Path) -> Result<RestoreReceipt> {
        self.inner.restore(id, destination)
    }
    fn reconcile(
        &self,
        base: &CheckpointRef,
        local: &Path,
        remote: &CheckpointRef,
    ) -> Result<Reconciliation> {
        self.inner.reconcile(base, local, remote)
    }
    fn accept_resolution(
        &self,
        id: &ReconciliationRef,
        paths: &[String],
    ) -> Result<Reconciliation> {
        self.inner.accept_resolution(id, paths)
    }
    fn apply(&self, id: &ReconciliationRef, options: ApplyOptions) -> Result<ApplyReceipt> {
        self.inner.apply(id, options)
    }
    fn inspect_apply(&self, id: &ReconciliationRef) -> Result<ApplyState> {
        self.inner.inspect_apply(id)
    }
    fn rollback(&self, id: &RecoveryRef, quiescent: bool) -> Result<()> {
        self.inner.rollback(id, quiescent)
    }
    fn export_tree_archive(&self, root: &Path) -> Result<Vec<u8>> {
        self.inner.export_tree_archive(root)
    }
    fn import_tree_archive(&self, bytes: &[u8], destination: &Path) -> Result<RestoreTreeReceipt> {
        self.inner.import_tree_archive(bytes, destination)
    }
    fn export_checkpoint_archive(&self, id: &CheckpointRef) -> Result<Vec<u8>> {
        self.inner.export_checkpoint_archive(id)
    }
    fn capture_archive(&self, bytes: &[u8], options: CaptureOptions) -> Result<Checkpoint> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.observed.lock().unwrap() = Some((bytes.to_vec(), options.clone()));
        if self.fail.load(Ordering::SeqCst) {
            return Err(Error::Unsupported(
                "legacy capture failure must propagate".into(),
            ));
        }
        self.inner.capture_archive(bytes, options)
    }
}
#[test]
fn compatibility_default_captures_once_with_default_options_and_propagates_errors() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let state = root.join("state");
    let engine = LegacyEngine {
        inner: GitCheckpointEngine::new("git", &state).unwrap(),
        calls: AtomicUsize::new(0),
        fail: AtomicBool::new(false),
        observed: Mutex::new(None),
    };
    let project = root.join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("file"), b"old implementation").unwrap();
    let archive = engine.export_tree_archive(&project).unwrap();
    let expected = engine.inner.fingerprint_archive(&archive).unwrap();
    assert_eq!(engine.fingerprint_archive(&archive).unwrap(), expected);
    assert_eq!(engine.calls.load(Ordering::SeqCst), 1);
    assert_eq!(artifacts(&state), 1);
    {
        let observed = engine.observed.lock().unwrap();
        let (bytes, options) = observed.as_ref().unwrap();
        assert_eq!(bytes, &archive);
        assert!(options.parent.is_none());
        assert!(options.environment_reference.is_none());
    }
    engine.fail.store(true, Ordering::SeqCst);
    assert!(
        matches!(engine.fingerprint_archive(&archive),Err(Error::Unsupported(message)) if message=="legacy capture failure must propagate")
    );
    assert_eq!(engine.calls.load(Ordering::SeqCst), 2);
    assert_eq!(artifacts(&state), 1);
}
