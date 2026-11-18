use super::*;

/// What the system's `ssh-keygen` derives from `private` as its public key.
fn derived_by_ssh_keygen(private: &[u8]) -> String {
    use std::os::unix::fs::OpenOptionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("key");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    std::io::Write::write_all(&mut file, private).unwrap();
    let derived = std::process::Command::new("ssh-keygen")
        .args(["-y", "-f"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(derived.status.success(), "{derived:?}");
    String::from_utf8(derived.stdout).unwrap().trim().to_owned()
}

#[tokio::test]
async fn a_key_made_in_memory_is_one_openssh_reads() {
    let pair = KeyPair::generate("made here").await.unwrap();
    assert_eq!(
        derived_by_ssh_keygen(pair.private()),
        format!("{} madehere", pair.public())
    );
    // Its own file and public line load back as the same pair.
    let loaded = KeyPair::new(pair.private().to_vec(), pair.public()).unwrap();
    assert_eq!(loaded, pair);
    // Each is new.
    let other = KeyPair::generate("made here").await.unwrap();
    assert_ne!(other.public(), pair.public());
}

/// The file is written into buffers made at their final size (checked by
/// `openssh_file`'s debug assertions), for every label length: a buffer that
/// grew would leave an unwiped copy of the key behind.
#[tokio::test]
async fn a_key_with_any_label_length_is_written_without_growing_a_buffer() {
    for length in [0, 1, 7, 8, 29, 30, 31, 63, 64, 200] {
        let label = "a".repeat(length);
        let pair = KeyPair::generate(&label).await.unwrap();
        let kept = "a".repeat(length.min(64));
        let derived = derived_by_ssh_keygen(pair.private());
        assert_eq!(
            derived.trim_end(),
            format!("{} {kept}", pair.public()).trim_end()
        );
        assert_eq!(
            KeyPair::new(pair.private().to_vec(), pair.public()).unwrap(),
            pair
        );
    }
}

#[test]
fn a_key_openssh_made_loads() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("key");
    let made = std::process::Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "a comment", "-f"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(made.success());
    let private = std::fs::read(&path).unwrap();
    let public = std::fs::read_to_string(path.with_extension("pub")).unwrap();
    let pair = KeyPair::new(private, &public).unwrap();
    assert_eq!(format!("{} a comment", pair.public()), public.trim());
}

#[tokio::test]
async fn a_pair_that_cannot_sign_in_is_refused_when_made() {
    let one = KeyPair::generate("one").await.unwrap();
    let two = KeyPair::generate("two").await.unwrap();
    // Halves of two keys.
    assert!(KeyPair::new(one.private().to_vec(), two.public()).is_err());
    assert!(KeyPair::new(b"not a key".to_vec(), one.public()).is_err());
    // A key behind a passphrase this SDK is never given.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("key");
    let made = std::process::Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "secret", "-f"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(made.success());
    let private = std::fs::read(&path).unwrap();
    let public = std::fs::read_to_string(path.with_extension("pub")).unwrap();
    assert!(KeyPair::new(private, public.trim()).is_err());
}

#[tokio::test]
async fn debug_never_shows_the_private_half() {
    let shown = format!("{:?}", KeyPair::generate("debug").await.unwrap());
    assert!(!shown.contains("PRIVATE"));
    assert!(!shown.contains("seed"));
}
