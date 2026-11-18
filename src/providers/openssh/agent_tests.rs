use super::*;
use crate::providers::openssh::KeyPair;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};

async fn request(stream: &mut UnixStream, body: &[u8]) -> Vec<u8> {
    let mut framed = Vec::new();
    put_string(&mut framed, body);
    stream.write_all(&framed).await.unwrap();
    let mut length = [0u8; 4];
    stream.read_exact(&mut length).await.unwrap();
    let mut reply = vec![0u8; u32::from_be_bytes(length) as usize];
    stream.read_exact(&mut reply).await.unwrap();
    reply
}

fn sign_request(blob: &[u8], data: &[u8]) -> Vec<u8> {
    let mut body = vec![SIGN_REQUEST];
    put_string(&mut body, blob);
    put_string(&mut body, data);
    body.extend_from_slice(&0u32.to_be_bytes());
    body
}

#[tokio::test]
async fn the_agent_lists_its_one_key_and_signs_only_with_it() {
    let pair = KeyPair::generate("agent-test").await.unwrap();
    let signer = Signer::new(pair.private(), pair.public()).unwrap();
    let blob = signer.blob.clone();
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("agent");
    let _agent = Agent::serve(signer, &socket).unwrap();

    // The system's own client sees exactly the key.
    let listed = tokio::process::Command::new("ssh-add")
        .arg("-L")
        .env("SSH_AUTH_SOCK", &socket)
        .output()
        .await
        .unwrap();
    assert!(listed.status.success());
    let listed = String::from_utf8(listed.stdout).unwrap();
    assert_eq!(listed.lines().count(), 1);
    assert!(listed.starts_with(pair.public()), "{listed}");

    let mut stream = UnixStream::connect(&socket).await.unwrap();
    let reply = request(&mut stream, &sign_request(&blob, b"session data")).await;
    assert_eq!(reply[0], SIGN_RESPONSE);
    let mut rest = &reply[1..];
    let mut inner = take_string(&mut rest).unwrap();
    assert_eq!(take_string(&mut inner).unwrap(), KEY_TYPE);
    let signature = Signature::from_slice(take_string(&mut inner).unwrap()).unwrap();
    let mut public = &blob[..];
    take_string(&mut public).unwrap();
    let public: [u8; 32] = take_string(&mut public).unwrap().try_into().unwrap();
    VerifyingKey::from_bytes(&public)
        .unwrap()
        .verify(b"session data", &signature)
        .unwrap();

    // Another key, a malformed request or any other kind is refused, and
    // the same stream keeps working.
    let other = KeyPair::generate("other").await.unwrap();
    let other = Signer::new(other.private(), other.public()).unwrap();
    for refused in [
        sign_request(&other.blob, b"x"),
        vec![SIGN_REQUEST, 0, 0],
        vec![27],
        vec![REQUEST_IDENTITIES, 0],
    ] {
        assert_eq!(request(&mut stream, &refused).await, [FAILURE]);
    }
    assert_eq!(
        request(&mut stream, &[REQUEST_IDENTITIES]).await[0],
        IDENTITIES_ANSWER
    );
}

#[tokio::test]
async fn an_oversized_request_closes_the_stream() {
    let pair = KeyPair::generate("agent-test").await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("agent");
    let _agent =
        Agent::serve(Signer::new(pair.private(), pair.public()).unwrap(), &socket).unwrap();
    let mut stream = UnixStream::connect(&socket).await.unwrap();
    stream
        .write_all(&((MAX_REQUEST + 1) as u32).to_be_bytes())
        .await
        .unwrap();
    let mut rest = Vec::new();
    stream.read_to_end(&mut rest).await.unwrap();
    assert!(rest.is_empty());
}

#[tokio::test]
async fn a_dropped_agent_stops_answering() {
    let pair = KeyPair::generate("agent-test").await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("agent");
    let agent = Agent::serve(Signer::new(pair.private(), pair.public()).unwrap(), &socket).unwrap();
    drop(agent);
    tokio::task::yield_now().await;
    let listed = tokio::process::Command::new("ssh-add")
        .arg("-L")
        .env("SSH_AUTH_SOCK", &socket)
        .output()
        .await
        .unwrap();
    assert!(!listed.status.success());
}

#[tokio::test]
async fn a_key_that_does_not_match_its_public_half_is_refused() {
    let one = KeyPair::generate("one").await.unwrap();
    let two = KeyPair::generate("two").await.unwrap();
    assert!(Signer::new(one.private(), two.public()).is_err());
    assert!(Signer::new(b"not a key", one.public()).is_err());
    // An encrypted key cannot be used without its passphrase.
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
    assert!(Signer::new(&private, public.trim()).is_err());
}
