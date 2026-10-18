//! Real native output bytes; text decoding and PTY emulation are not involved.
// Reuse the ownership/lease cleanup fixture; continuation helpers are unused here.
#[allow(dead_code)]
#[path = "support/recovery_continuation_fixture.rs"]
mod fixture;
mod support;
use fixture::*;
use std::time::Duration;
use workspace_sdk::*;

async fn binary_run(fixture: &Fixture, budget: u64) -> Run {
    let workspace = fixture
        .sdk
        .open(
            &host("laptop"),
            OpenRequest {
                workspace_id: WorkspaceId::new("binary-output").unwrap(),
                config: ProviderOptions::default(),
                resources: ResourceRequirements::default(),
            },
        )
        .await
        .unwrap();
    let script = "import os; os.write(1,bytes(range(256))+'€🦀'.encode()); os.write(2,bytes(reversed(range(256)))+'🦀€'.encode())";
    let execution = CommandSpec::new("/usr/bin/python3")
        .args(["-c", script])
        .into();
    workspace
        .start_with(LaunchRequest {
            id: RunId::new("binary-claim").unwrap(),
            execution,
            output_limit_bytes: budget,
        })
        .await
        .unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn binary_pages_preserve_every_byte_and_independent_cursors_across_reconnect() {
    run_case("binary-output-pages", |fixture| async move {
        let run = binary_run(&fixture, 1024).await;
        assert_eq!(
            run.wait(Duration::from_secs(10)).await.unwrap(),
            RunState::Exited { exit_code: 0 }
        );
        let reference = run.reference();
        let first = run.output_bytes(OutputCursor::default(), 2).await.unwrap();
        assert_eq!(first.stdout, vec![0]);
        assert_eq!(first.stderr, vec![255]);
        assert_eq!((first.next.stdout, first.next.stderr), (1, 1));
        assert!(!first.complete);
        assert!(!first.gap);
        drop(run);
        let reconnected = fixture
            .sdk
            .connect(&reference.workspace)
            .await
            .unwrap()
            .run(reference.id.clone());
        let repeated = reconnected
            .output_bytes(OutputCursor::default(), 2)
            .await
            .unwrap();
        assert_eq!(repeated.stdout, first.stdout);
        assert_eq!(repeated.stderr, first.stderr);
        assert_eq!((repeated.next.stdout, repeated.next.stderr), (1, 1));
        let independent = reconnected
            .output_bytes(
                OutputCursor {
                    stdout: 260,
                    stderr: 0,
                },
                3,
            )
            .await
            .unwrap();
        assert_eq!(independent.stdout, vec![159]);
        assert_eq!(independent.stderr, vec![255, 254]);
        assert_eq!((independent.next.stdout, independent.next.stderr), (261, 2));
        assert!(!independent.gap);
        assert!(!independent.complete);
        let mut stdout = first.stdout;
        let mut stderr = first.stderr;
        let mut cursor = first.next;
        for position in 1..263 {
            let page = reconnected.output_bytes(cursor, 2).await.unwrap();
            assert!(!page.gap);
            assert_eq!(page.stdout.len(), 1);
            assert_eq!(page.stderr.len(), 1);
            stdout.extend(page.stdout);
            stderr.extend(page.stderr);
            assert_eq!(
                (page.next.stdout, page.next.stderr),
                (position + 1, position + 1)
            );
            assert_eq!(page.complete, position == 262);
            cursor = page.next;
        }
        let mut expected_stdout = (0..=255).collect::<Vec<u8>>();
        expected_stdout.extend("€🦀".as_bytes());
        assert_eq!(stdout, expected_stdout);
        let mut expected_stderr = (0..=255).rev().collect::<Vec<u8>>();
        expected_stderr.extend("🦀€".as_bytes());
        assert_eq!(stderr, expected_stderr);
        let drained = reconnected.output_bytes(cursor, 2).await.unwrap();
        assert!(drained.complete);
        assert!(!drained.gap);
        assert!(drained.stdout.is_empty());
        assert!(drained.stderr.is_empty());
        assert_eq!((drained.next.stdout, drained.next.stderr), (263, 263));
        assert_eq!(reconnected.reference(), reference);
        assert_eq!(fixture.run_count().await, 1);
    })
    .await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn binary_retention_reports_gaps_and_refuses_invalid_limits_cursors_and_targets() {
    run_case("binary-output-retention", |fixture| async move {
        let run = binary_run(&fixture, 8).await;
        assert_eq!(
            run.wait(Duration::from_secs(10)).await.unwrap(),
            RunState::Exited { exit_code: 0 }
        );
        let page = run.output_bytes(OutputCursor::default(), 8).await.unwrap();
        assert!(page.gap);
        assert!(page.complete);
        assert_eq!(page.stdout, vec![240, 159, 166, 128]);
        assert_eq!(page.stderr, vec![128, 226, 130, 172]);
        assert_eq!((page.next.stdout, page.next.stderr), (263, 263));
        let replay = run.output_bytes(OutputCursor::default(), 8).await.unwrap();
        assert!(replay.gap);
        assert_eq!(replay.stdout, page.stdout);
        assert_eq!(replay.stderr, page.stderr);
        for limit in [0, 1, 262145] {
            assert!(matches!(
                run.output_bytes(OutputCursor::default(), limit).await,
                Err(Error::InvalidInput(_))
            ));
        }
        for future in [
            OutputCursor {
                stdout: 264,
                stderr: 263,
            },
            OutputCursor {
                stdout: 263,
                stderr: 264,
            },
        ] {
            assert!(matches!(
                run.output_bytes(future, 8).await,
                Err(Error::InvalidInput(_))
            ));
        }
        let other = fixture
            .sdk
            .open(
                &host("laptop"),
                OpenRequest {
                    workspace_id: WorkspaceId::new("other-output-target").unwrap(),
                    config: ProviderOptions::default(),
                    resources: ResourceRequirements::default(),
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            other
                .backend()
                .read_output_bytes(&run.reference(), OutputCursor::default(), 8)
                .await,
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(fixture.run_count().await, 1);
    })
    .await;
}
