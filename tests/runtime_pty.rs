#![cfg(unix)]
//! Public runtime-host acceptance against a real PTY and persistent supervisor.
#[path = "support/pty_fixture.rs"]
mod fixture;
use fixture::{assert_no_secret_files, input, run_case, Transcript, ENV_SECRET, INPUT_SECRET};
use std::{
    fs,
    io::{BufRead, BufReader},
    os::unix::net::UnixListener,
    path::Path,
    process::Command,
    time::Duration,
};
use tokio::time::{sleep, timeout};
use workspace_sdk::{runtime::*, Error, OutputCursor, SecretInput};

#[tokio::test]
async fn terminal_input_resize_and_reconnect_preserve_scope_and_transient_credentials() {
    run_case(1, |fixtures| async move {
        let fixture = &fixtures[0];
        assert!(
            std::env::var_os("PATH").is_some(),
            "fixture must remove an inherited variable"
        );
        for private in [false, true] {
            let host = fixture.host().await;
            let cwd = if private {
                let path = format!("{}/runtime/pty-private", fixture.descriptor.control_root);
                host.ensure_private_directory(&path).await.unwrap();
                path
            } else {
                fixture.descriptor.root.clone()
            };
            let mut command = fixture.command(&cwd, &["interactive"]);
            command
                .environment
                .insert("PTY_PUBLIC_VALUE".into(), "visible-fixture".into());
            command
                .secret_environment
                .insert("PTY_PRIVATE_VALUE".into(), SecretValue::new(ENV_SECRET));
            command.remove_environment.push("PATH".into());
            assert!(!format!("{command:?}").contains(ENV_SECRET));
            let key = if private {
                "private-tty"
            } else {
                "project-tty"
            };
            let process = fixture.start(&host, key, command.clone()).await;
            let mut output = Transcript::default();
            output.until(&host, &process, "\nready\n").await;
            let prefix =
                format!("tty:111\nforeground:yes\ncwd:{cwd}\nenv:ok\nstderr:ready\nready\n");
            assert_eq!(output.text, prefix);
            assert_eq!(
                host.start_interactive(key, command.clone(), true)
                    .await
                    .unwrap(),
                process
            );
            let mut rebound = command.clone();
            rebound.args.push("different-command".into());
            assert!(matches!(
                host.start_interactive(key, rebound, true).await,
                Err(Error::Conflict(_))
            ));
            assert_eq!(
                fs::read_to_string(Path::new(&cwd).join("launches")).unwrap(),
                "launch\n"
            );
            input(&host, &process, INPUT_SECRET.as_bytes()).await;
            output.until(&host, &process, "input:accepted\n").await;
            let saved_process = serde_json::to_vec(&process).unwrap();
            let saved_cursor = serde_json::to_vec(&output.cursor).unwrap();
            drop(host); // Fixture owns no backend or host connection.

            let host = fixture.host().await;
            let process: RuntimeProcessRef = serde_json::from_slice(&saved_process).unwrap();
            output.cursor = serde_json::from_slice(&saved_cursor).unwrap();
            assert_eq!(
                host.inspect_process(&process).await.unwrap(),
                RuntimeProcessState::Running
            );
            assert_eq!(
                host.start_interactive(key, command.clone(), true)
                    .await
                    .unwrap(),
                process
            );
            host.resize_process(&process, RuntimeTerminalSize::new(120, 40).unwrap())
                .await
                .unwrap();
            input(&host, &process, b"size\n").await;
            output.until(&host, &process, "size:120x40\n").await;
            input(&host, &process, b"quit\n").await;
            output.complete(&host, &process).await;
            assert_eq!(
                host.inspect_process(&process).await.unwrap(),
                RuntimeProcessState::Exited { code: 0 }
            );
            assert_eq!(
                output.text,
                format!("{prefix}input:accepted\nsize:120x40\nstdout:done\nstderr:done\n")
            );
            assert!(!output.text.contains(INPUT_SECRET.trim_end()));
            assert!(!output.text.contains(ENV_SECRET));
            assert_eq!(output.cursor.stdout, output.text.len() as u64);
            assert_eq!(
                host.start_interactive(key, command, true).await.unwrap(),
                process
            );
            assert_eq!(
                fs::read_to_string(Path::new(&cwd).join("launches")).unwrap(),
                "launch\n"
            );
            let end = host
                .read_process_output(&process, output.cursor.clone(), 128)
                .await
                .unwrap();
            assert!(end.complete && end.stdout.is_empty() && end.stderr.is_empty() && !end.gap);
            assert_eq!(end.next.stdout, output.cursor.stdout);
            assert_eq!(end.next.stderr, 0);
            assert_no_secret_files(Path::new(&fixture.descriptor.control_root));
        }
    })
    .await;
}

#[tokio::test]
async fn terminal_controls_reject_another_workspace_and_cancel_the_owned_process() {
    run_case(2, |fixtures| async move {
        let left = fixtures[0].host().await;
        let right = fixtures[1].host().await;
        let first = fixtures[0]
            .start(
                &left,
                "same-attempt",
                fixtures[0].command(&fixtures[0].descriptor.root, &["control"]),
            )
            .await;
        let second = fixtures[1]
            .start(
                &right,
                "same-attempt",
                fixtures[1].command(&fixtures[1].descriptor.root, &["control"]),
            )
            .await;
        assert_eq!(first.id(), second.id());
        assert_ne!(first.host_scope(), second.host_scope());
        let mut left_output = Transcript::default();
        let mut right_output = Transcript::default();
        left_output.until(&left, &first, "\nready\n").await;
        right_output.until(&right, &second, "\nready\n").await;
        assert!(matches!(
            right.inspect_process(&first).await,
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            right
                .read_process_output(&first, OutputCursor::default(), 128)
                .await,
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            right
                .write_process_input(&first, &SecretInput::new(b"quit\n".to_vec()))
                .await,
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            right
                .resize_process(&first, RuntimeTerminalSize::new(132, 55).unwrap())
                .await,
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            right.cancel_process(&first).await,
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(
            left.inspect_process(&first).await.unwrap(),
            RuntimeProcessState::Running
        );
        assert_eq!(
            right.inspect_process(&second).await.unwrap(),
            RuntimeProcessState::Running
        );
        input(&right, &second, b"size\n").await;
        right_output.until(&right, &second, "size:80x24\n").await;
        assert!(!right_output.text.contains("132x55"));
        assert_eq!(
            left.cancel_process(&first).await.unwrap(),
            RuntimeProcessState::Exited { code: 130 }
        );
        left_output.complete(&left, &first).await;
        assert_eq!(
            right.inspect_process(&second).await.unwrap(),
            RuntimeProcessState::Running
        );
        assert_eq!(
            right.cancel_process(&second).await.unwrap(),
            RuntimeProcessState::Exited { code: 130 }
        );
        right_output.complete(&right, &second).await;
    })
    .await;
}

#[tokio::test]
async fn retained_terminal_output_is_an_exact_merged_tail_with_absolute_cursors_and_eof() {
    run_case(1, |fixtures| async move {
        let fixture = &fixtures[0];
        let host = fixture.host().await;
        let process = fixture
            .start(
                &host,
                "bounded-output",
                fixture.command(&fixture.descriptor.root, &["burst"]),
            )
            .await;
        timeout(Duration::from_secs(10), async {
            while host.inspect_process(&process).await.unwrap() == RuntimeProcessState::Running {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            host.inspect_process(&process).await.unwrap(),
            RuntimeProcessState::Exited { code: 0 }
        );
        let mut expected = b"stdout-prefix\nstderr-prefix\n".to_vec();
        expected.extend((0..300_000).map(|index| b'a' + (index % 26) as u8));
        expected.extend_from_slice(b"\nstdout-tail\nstderr-tail\n");
        let mut cursor = OutputCursor::default();
        let mut retained = Vec::new();
        let mut first_offset = None;
        timeout(Duration::from_secs(10), async {
            loop {
                let page = host
                    .read_process_output(&process, cursor.clone(), 4096)
                    .await
                    .unwrap();
                assert!(page.stderr.is_empty());
                assert_eq!(page.next.stderr, 0);
                assert!(page.stdout.len() <= 4096);
                if first_offset.is_none() {
                    assert!(
                        page.gap,
                        "output larger than the 128 KiB budget must report loss"
                    );
                    assert!(!page.stdout.is_empty());
                    let first = page.next.stdout - page.stdout.len() as u64;
                    assert!(first > 0);
                    first_offset = Some(first);
                } else {
                    assert!(
                        !page.gap,
                        "a stable finished transcript cannot invent another gap"
                    );
                    assert_eq!(page.next.stdout, cursor.stdout + page.stdout.len() as u64);
                }
                retained.extend_from_slice(page.stdout.as_bytes());
                cursor = page.next;
                if page.complete {
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(cursor.stdout, expected.len() as u64);
        assert!(!retained.is_empty() && retained.len() <= 128 * 1024);
        assert_eq!(retained, expected[first_offset.unwrap() as usize..]);
        drop(host);
        let host = fixture.host().await;
        let end = host
            .read_process_output(&process, cursor.clone(), 128)
            .await
            .unwrap();
        assert!(end.complete && end.stdout.is_empty() && end.stderr.is_empty() && !end.gap);
        assert_eq!(end.next.stdout, cursor.stdout);
        assert_eq!(end.next.stderr, 0);
        let replay = host
            .read_process_output(&process, OutputCursor::default(), 4096)
            .await
            .unwrap();
        assert!(replay.gap);
        assert_eq!(replay.stdout.as_bytes(), &retained[..replay.stdout.len()]);
        assert_eq!(
            replay.next.stdout - replay.stdout.len() as u64,
            first_offset.unwrap()
        );
    })
    .await;
}

#[tokio::test]
async fn leader_exit_settles_a_descendant_that_ignores_hup_and_holds_the_terminal() {
    run_case(1, |fixtures| async move {
        let fixture = &fixtures[0];
        let socket_path = fixture.directory.path().join("descendant-control.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let host = fixture.host().await;
        let process = fixture
            .start(
                &host,
                "held-terminal",
                fixture.command(
                    &fixture.descriptor.root,
                    &["hold-leader", socket_path.to_str().unwrap()],
                ),
            )
            .await;
        let connection = timeout(Duration::from_secs(5), async {
            loop {
                match listener.accept() {
                    Ok((connection, _)) => break connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        sleep(Duration::from_millis(10)).await
                    }
                    Err(error) => panic!("fixture control accept failed: {error}"),
                }
            }
        })
        .await
        .unwrap();
        connection
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut control = BufReader::new(connection);
        let mut ready = String::new();
        control.read_line(&mut ready).unwrap();
        let descendant: u32 = ready.trim().parse().unwrap();
        let mut output = Transcript::default();
        // A live descendant keeps the slave open beyond the bounded drain.
        // Completion must preserve this uncertainty even though retained bytes
        // are contiguous and the owner settles the descendant before publishing.
        timeout(Duration::from_secs(10), async {
            loop {
                let page = host
                    .read_process_output(&process, output.cursor.clone(), 4096)
                    .await
                    .unwrap();
                assert!(page.stderr.is_empty());
                assert_eq!(page.next.stderr, 0);
                assert_eq!(
                    page.next.stdout,
                    output.cursor.stdout + page.stdout.len() as u64
                );
                output.text.push_str(&page.stdout);
                output.cursor = page.next;
                if page.complete {
                    assert!(page.gap, "bounded drain must remain visibly incomplete");
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("bounded drain and owned-process cleanup must complete");
        // No grace period after terminal publication: cleanup must already hold.
        let state = Command::new("/bin/ps")
            .args(["-p", &descendant.to_string(), "-o", "stat="])
            .output()
            .unwrap();
        assert!(
            state.status.success() || state.status.code() == Some(1),
            "ps failed"
        );
        assert!(
            state.stderr.is_empty(),
            "ps could not establish process liveness"
        );
        let state = String::from_utf8(state.stdout).unwrap();
        assert!(
            state.trim().is_empty() || state.trim().starts_with('Z'),
            "owned descendant must already be stopped at terminal publication"
        );
        assert_eq!(output.text, "leader-complete\n");
        assert_eq!(output.cursor.stdout, "leader-complete\n".len() as u64);
        assert_eq!(
            host.inspect_process(&process).await.unwrap(),
            RuntimeProcessState::Exited { code: 0 }
        );
        let reconnected = fixture.host().await;
        for reader in [&host, &reconnected] {
            let end = reader
                .read_process_output(&process, output.cursor.clone(), 4096)
                .await
                .unwrap();
            assert!(end.complete && end.gap);
            assert!(end.stdout.is_empty() && end.stderr.is_empty());
            assert_eq!(end.next.stdout, output.cursor.stdout);
            assert_eq!(end.next.stderr, output.cursor.stderr);
        }
        let replay = reconnected
            .read_process_output(&process, OutputCursor::default(), 4096)
            .await
            .unwrap();
        assert!(replay.complete && replay.gap);
        assert_eq!(replay.stdout, "leader-complete\n");
        assert!(replay.stderr.is_empty());
        assert_eq!(replay.next.stdout, output.cursor.stdout);
        assert_eq!(replay.next.stderr, output.cursor.stderr);
        assert!(!Path::new(&fixture.descriptor.root)
            .join("descendant-escaped")
            .exists());
        // Keep the independent cleanup channel alive through every liveness check.
        drop(control);
    })
    .await;
}
