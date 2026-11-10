use super::*;

#[test]
fn a_script_with_its_own_quotes_reaches_sh_whole() {
    let script = posix("echo 'a  b'; echo \"$(echo c)\"");
    // The machine's sshd hands the command to the account's login shell.
    let said = std::process::Command::new("bash")
        .args(["-c", &script])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&said.stdout), "a  b\nc\n");
}

#[test]
fn only_one_plain_ed25519_host_key_is_pinned() {
    let key = "AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";
    assert_eq!(
        known_host(Some(&format!("ssh-ed25519 {key}"))),
        Some(format!("{HOST_ALIAS} ssh-ed25519 {key}\n"))
    );
    assert_eq!(
        known_host(Some(&format!("ssh-ed25519 {key} comment"))),
        Some(format!("{HOST_ALIAS} ssh-ed25519 {key}\n"))
    );
    for bad in [
        None,
        Some(""),
        Some("ssh-ed25519"),
        Some("ssh-rsa AAAA"),
        Some("ssh-ed25519 not*base64"),
        Some("ssh-ed25519 AAAA\nevil ssh-ed25519 AAAA"),
    ] {
        assert_eq!(known_host(bad), None, "{bad:?}");
    }
    let long = "A".repeat(260);
    assert_eq!(known_host(Some(&format!("ssh-ed25519 {long}"))), None);
}

#[tokio::test]
async fn a_long_output_keeps_its_end() {
    let bytes: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
    let tail = read_tail(&bytes[..], 1000).await.unwrap();
    assert_eq!(tail, bytes[9000..]);
    assert_eq!(read_tail(&b"short"[..], 1000).await.unwrap(), b"short");
}

/// A real OpenSSH server stands in for the machine and a fake Boat
/// authorizes the key. It needs `/usr/sbin/sshd`, `python3` and an account
/// named `user`, so it runs only when asked:
/// `cargo test --features boat connection -- --ignored`.
#[tokio::test]
#[ignore = "needs a local sshd, python3 and an account named user"]
async fn a_connection_streams_runs_serves_files_and_forwards_one_socket() {
    use axum::{routing::post, Json, Router};
    use serde_json::{json, Value};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    use std::os::unix::fs::PermissionsExt;
    let place = tempfile::tempdir().unwrap();
    // sshd reads the authorized keys as `user`.
    std::fs::set_permissions(place.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = |name: &str| place.path().join(name);
    let made = std::process::Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(path("host"))
        .status()
        .unwrap();
    assert!(made.success());
    let host_key = std::fs::read_to_string(path("host.pub")).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    std::fs::write(path("authorized"), "").unwrap();
    std::fs::write(
        path("sshd_config"),
        format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nAuthorizedKeysFile {}\n\
             StrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\n\
             UsePAM no\nPidFile none\nAllowStreamLocalForwarding yes\n\
             Subsystem sftp /usr/lib/openssh/sftp-server\n",
            path("host").display(),
            path("authorized").display()
        ),
    )
    .unwrap();
    let mut sshd = tokio::process::Command::new("/usr/sbin/sshd")
        .args(["-D", "-e", "-f"])
        .arg(path("sshd_config"))
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    while tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_err()
    {
        assert!(sshd.try_wait().unwrap().is_none(), "sshd did not start");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    let authorized = path("authorized");
    let authorized_path = authorized.clone();
    let answer = json!({
        "ok": true, "type": "sandbox.sshkey", "success": true, "sshUser": "user",
        "sshEndpoint": format!("127.0.0.1:{port}"), "hostKey": host_key.trim(),
    });
    let router = Router::new().route(
        "/api/v1/sandboxes/{id}/sshkey",
        post(move |Json(request): Json<Value>| {
            // Written in place, so a mode the test set is kept.
            let _ = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&authorized)
                .and_then(|mut file| {
                    std::io::Write::write_all(
                        &mut file,
                        request["key"].as_str().unwrap().as_bytes(),
                    )
                });
            let answer = answer.clone();
            async move { Json(answer) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let api = Api::new(
        sandbox_client::ClientConfig::new("fixture-token")
            .unwrap()
            .with_base_path(&base)
            .unwrap(),
    )
    .unwrap();

    // The local socket answers each line it is sent with that line, marked.
    let local = path("local.sock");
    let listening = tokio::net::UnixListener::bind(&local).unwrap();
    let echo = tokio::spawn(async move {
        loop {
            let (socket, _) = listening.accept().await.unwrap();
            tokio::spawn(async move {
                let (read, mut write) = socket.into_split();
                let mut lines = BufReader::new(read).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    write
                        .write_all(format!("local:{line}\n").as_bytes())
                        .await
                        .unwrap();
                }
            });
        }
    });

    // A key the machine refuses fails with what ssh said.
    std::fs::set_permissions(&authorized_path, std::fs::Permissions::from_mode(0o000)).unwrap();
    let refused = SshConnection::connect(&api, "bx_23456789", None).await;
    let Err(Error::Provider { message, .. }) = refused else {
        panic!("a refused key connected");
    };
    assert!(message.contains("Permission denied"), "{message}");
    std::fs::set_permissions(&authorized_path, std::fs::Permissions::from_mode(0o644)).unwrap();

    let connection = SshConnection::connect(
        &api,
        "bx_23456789",
        Some(SocketForward {
            local: local.clone(),
        }),
    )
    .await
    .unwrap();
    let private = connection.directory.path().to_owned();

    assert_eq!(
        connection
            .run("echo 'it'\"'\"'s'; whoami", None)
            .await
            .unwrap(),
        "it's\nuser\n"
    );

    let mut stream = connection
        .clone()
        .stream(
            "IFS= read -r t; echo \"first:$t\"; cat",
            Some(b"SENTINEL_SECRET"),
        )
        .await
        .unwrap();
    stream.input.write_all(b"after\n").await.unwrap();
    drop(stream.input);
    let mut said = String::new();
    stream.output.read_to_string(&mut said).await.unwrap();
    assert_eq!(said, "first:SENTINEL_SECRET\nafter\n");
    drop(stream.keep);

    let remote = connection.forwarded_socket().unwrap().to_owned();
    let relayed = connection
        .run(
            &format!(
                "python3 -c 'import socket,sys; s=socket.socket(socket.AF_UNIX); \
                 s.connect(sys.argv[1]); s.sendall(b\"ping\\n\"); \
                 print(s.makefile().readline(), end=\"\")' {remote}"
            ),
            None,
        )
        .await
        .unwrap();
    assert_eq!(relayed, "local:ping\n");

    // SSH_FXP_INIT, version 3, answered by SSH_FXP_VERSION.
    let mut files = connection.clone().files().await.unwrap();
    files
        .to_machine
        .write_all(&[0, 0, 0, 5, 1, 0, 0, 0, 3])
        .await
        .unwrap();
    let mut header = [0u8; 5];
    files.from_machine.read_exact(&mut header).await.unwrap();
    assert_eq!(header[4], 2);
    drop(files);

    drop(connection);
    assert!(!private.exists(), "the key and socket files outlived it");
    echo.abort();
    server.abort();
    sshd.kill().await.unwrap();
}
