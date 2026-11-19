use super::*;

fn age(path: &Path, by: Duration) {
    let then = SystemTime::now() - by;
    File::open(path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(then))
        .unwrap();
}

#[test]
fn a_directory_whose_owner_is_gone_is_swept_and_a_live_one_kept() {
    let parent = tempfile::tempdir().unwrap();
    let live = ConnectionDirectory::make_in(parent.path()).unwrap();
    // A process killed outright: the system releases its lock, and the
    // directory and its files stay. The lock file is made here and never
    // taken. Taking it and dropping it would not model that: a test
    // spawning a process at that moment can hold a forked copy of the
    // descriptor, and with it the lock, until its exec.
    let killed_path = parent.path().join(format!("{PREFIX}killed"));
    std::fs::create_dir(&killed_path).unwrap();
    File::create(killed_path.join("lock")).unwrap();
    std::fs::write(killed_path.join("hosts"), "").unwrap();
    // One made by an older build: no lock, a key, and no master answering.
    let old = parent.path().join(format!("{PREFIX}old"));
    std::fs::create_dir(&old).unwrap();
    std::fs::write(old.join("key"), "secret").unwrap();
    // Something else entirely.
    let other = parent.path().join("other");
    std::fs::create_dir(&other).unwrap();

    for path in [live.path(), &killed_path, &old, &other] {
        age(path, Duration::from_secs(600));
    }
    sweep(parent.path(), SWEEP_AFTER);
    assert!(live.path().exists());
    assert!(!killed_path.exists());
    assert!(!old.exists());
    assert!(other.exists());
}

#[test]
fn a_young_directory_is_kept_even_without_its_lock() {
    let parent = tempfile::tempdir().unwrap();
    let young = parent.path().join(format!("{PREFIX}young"));
    std::fs::create_dir(&young).unwrap();
    sweep(parent.path(), SWEEP_AFTER);
    assert!(young.exists());
}

#[test]
fn an_old_directory_whose_master_answers_is_kept() {
    let parent = tempfile::tempdir().unwrap();
    let old = parent.path().join(format!("{PREFIX}serving"));
    std::fs::create_dir(&old).unwrap();
    let _master = std::os::unix::net::UnixListener::bind(old.join("control")).unwrap();
    age(&old, Duration::from_secs(600));
    sweep(parent.path(), SWEEP_AFTER);
    assert!(old.exists());
}

#[test]
fn a_link_is_never_followed() {
    let parent = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    std::fs::write(target.path().join("keep"), "").unwrap();
    std::os::unix::fs::symlink(target.path(), parent.path().join(format!("{PREFIX}link"))).unwrap();
    sweep(parent.path(), Duration::ZERO);
    assert!(target.path().join("keep").exists());
}
