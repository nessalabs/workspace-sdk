//! Opt-in probes. The transient failures are injected locally; successful requests
//! go to the live service. The handoff probe creates two short-lived fixture Sandboxes.
#![cfg(unix)]
use boat_client::{handoff::*, *};
use std::{path::PathBuf, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type ProbeResult<T> = std::result::Result<T, std::boxed::Box<dyn std::error::Error + Send + Sync>>;
fn api() -> BoatApi {
    BoatApi::new(
        BoatClientConfig::from_env()
            .expect("live configuration")
            .with_retry(RetryConfig {
                max_attempts: 4,
                initial_delay: Duration::from_millis(20),
                max_delay: Duration::from_millis(100),
                ..Default::default()
            }),
    )
    .expect("client")
}

#[tokio::test]
#[ignore = "requires BOAT_API_KEY; local fault injection with a live read, creates no resources"]
async fn live_read_after_injected_transient_failures() -> ProbeResult<()> {
    let upstream = api();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        for attempt in 0..3 {
            let (mut stream, _) = listener.accept().await?;
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).await?;
                request.push(byte[0]);
                if request.len() > 16_384 {
                    return Err("request too large".into());
                }
            }
            let (status, body) = if attempt < 2 {
                (
                    503,
                    b"{\"error\":{\"code\":\"injected_transient\"}}".to_vec(),
                )
            } else {
                // Only the upstream client uses the real credential. Never forward
                // arbitrary proxy paths or print account data.
                (200, serde_json::to_vec(&upstream.me().await?)?)
            };
            stream.write_all(format!("HTTP/1.1 {status} response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nRetry-After: 0\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await?;
            stream.write_all(&body).await?;
        }
        Ok::<_, std::boxed::Box<dyn std::error::Error + Send + Sync>>(())
    });
    let through_faults = BoatApi::new(
        BoatClientConfig::new("local-proxy-placeholder")?
            .with_base_path(format!("http://{address}"))?
            .with_retry(RetryConfig {
                max_attempts: 4,
                initial_delay: Duration::from_millis(20),
                jitter: false,
                ..Default::default()
            }),
    )?;
    let response = tokio::time::timeout(Duration::from_secs(90), through_faults.me()).await??;
    if !response.ok {
        return Err("live me response was not successful".into());
    }
    task.await??;
    eprintln!("PASS: two injected 503s followed by a successful authenticated live read");
    Ok(())
}

#[tokio::test]
#[ignore = "requires BOAT_API_KEY, BOAT_TEST_MUTATIONS=1 and BOAT_TEST_STATE_DIR; creates and deletes two fixture Sandboxes"]
async fn live_handoff_reconciles_completed_launch_and_cleans_up() -> ProbeResult<()> {
    if std::env::var("BOAT_TEST_MUTATIONS").as_deref() != Ok("1") {
        return Err("BOAT_TEST_MUTATIONS=1 required".into());
    }
    let directory = PathBuf::from(std::env::var("BOAT_TEST_STATE_DIR")?);
    std::fs::create_dir_all(&directory)?;
    let api = api();
    if !api.limits(&Default::default()).await?.can_start {
        return Err("account cannot start a fixture Sandbox".into());
    }
    let unique = format!("rust-recovery-{:016x}", fastrand::u64(..));
    let mut sandboxes = Vec::new();
    let result = async {
        for suffix in ["source", "target"] {
            let creation_key = format!("{unique}-{suffix}");
            // Keep creation keys even if the response is lost, so an operator can
            // reconcile the exact create request without inventing another key.
            std::fs::write(directory.join(format!("{creation_key}.create-key")), &creation_key)?;
            let created = api.create(Some(CreateSandboxRequest {
                no_env: Some(true), ..CreateSandboxRequest::ttl(300)
            }), &boat_client::CreateOptions { idempotency_key: Some(creation_key.clone()), ..Default::default() }).await?;
            sandboxes.push(created.sandbox.id.clone());
            eprintln!("created {suffix} fixture={}", created.sandbox.id);
            std::fs::write(directory.join(format!("{unique}.sandboxes.json")), serde_json::to_vec(&sandboxes)?)?;
            wait_until_ready(&api, &created.sandbox.id).await?;
        }
        let source = &sandboxes[0]; let target = &sandboxes[1];
        let source_run = api.exec(source, "printf '3' > /tmp/boat-client-recovery-counter").await?;
        if source_run.exit_code != Some(0) { return Err("source command failed".into()); }
        let journal = HandoffJournal::new(FileReceiptStore::new(directory.join("receipts"))?);
        let mut r = journal.create(&unique, HandoffPlan { source_sandbox_id: source.clone(), target_sandbox_id: target.clone(),
            intent: "counter=3; restore; add=2; verify-single-launch-v1".into() }).await?;
        r = journal.advance(&r, HandoffTransition::BeginCapture).await?;
        let captured = read_text(&api, source, "/tmp/boat-client-recovery-counter").await?;
        if captured != "3" { return Err("source checkpoint mismatch".into()); }
        let checkpoint = directory.join(format!("{unique}.checkpoint"));
        std::fs::write(&checkpoint, captured)?;
        std::fs::File::open(&checkpoint)?.sync_all()?;
        r = journal.advance(&r, HandoffTransition::Captured(CheckpointReference { reference: checkpoint.to_string_lossy().into(), digest: None })).await?;
        r = journal.advance(&r, HandoffTransition::BeginRestore).await?;
        write_text(&api, target, "/tmp/boat-client-recovery-counter", &std::fs::read_to_string(&checkpoint)?).await?;
        // Simulate losing the controller after the remote restore but before its
        // confirmation. A new journal observes Restoring and checks the target.
        drop(journal);
        let journal = HandoffJournal::new(FileReceiptStore::new(directory.join("receipts"))?);
        let restored = journal.load(&unique).await?;
        if restored != r || restored.phase() != HandoffPhase::Restoring { return Err("restore receipt lost".into()); }
        if read_text(&api, target, "/tmp/boat-client-recovery-counter").await? != "3" { return Err("restore verification failed".into()); }
        r = journal.advance(&restored, HandoffTransition::Restored).await?;
        r = journal.advance(&r, HandoffTransition::BeginStart).await?;
        // This is intentionally executed once. Recovery uses the remote marker,
        // not another launch. Launch count makes duplication observable.
        let command = api.exec(target, "n=$(cat /tmp/boat-client-recovery-counter); printf '%s' \"$((n + 2))\" > /tmp/boat-client-recovery-counter; printf 'launch\\n' >> /tmp/boat-client-recovery-launches; printf 'complete' > /tmp/boat-client-recovery-completed").await?;
        if command.exit_code != Some(0) { return Err("continuation command failed".into()); }
        drop(journal); // Remote launch succeeded, Started was never persisted.
        let journal = HandoffJournal::new(FileReceiptStore::new(directory.join("receipts"))?);
        let pending = journal.load(&unique).await?;
        if pending != r || !pending.phase().needs_reconciliation() { return Err("pending launch was not preserved".into()); }
        if read_text(&api, target, "/tmp/boat-client-recovery-completed").await? != "complete" { return Err("cannot confirm remote launch".into()); }
        let done = journal.advance(&pending, HandoffTransition::Started { run_id: format!("{unique}-continuation") }).await?;
        if read_text(&api, target, "/tmp/boat-client-recovery-counter").await? != "5" { return Err("counter should be 5".into()); }
        if read_text(&api, target, "/tmp/boat-client-recovery-launches").await? != "launch\n" { return Err("continuation ran more than once".into()); }
        if journal.create(&unique, done.plan().clone()).await? != done { return Err("receipt replay changed completed operation".into()); }
        eprintln!("PASS: source 3 -> target 5; restart after restore and launch reconciled; launch count=1");
        Ok::<_, std::boxed::Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    eprintln!("handoff outcome={result:?}");
    let mut deletions = Vec::new();
    let mut cleanup_failed = false;
    for sandbox_id in &sandboxes {
        match api.delete_sandbox(sandbox_id).await {
            Ok(deletion) => {
                eprintln!(
                    "cleanup box={sandbox_id} operation={}",
                    deletion.operation.id
                );
                deletions.push((sandbox_id.clone(), deletion.operation.id));
            }
            Err(error) => {
                eprintln!("delete failed box={sandbox_id}: {error}");
                cleanup_failed = true;
            }
        }
    }
    std::fs::write(
        directory.join(format!("{unique}.deletions.json")),
        serde_json::to_vec(&deletions)?,
    )?;
    let api_ref = &api;
    let outcomes = futures_util::future::join_all(deletions.iter().map(
        |(sandbox_id, operation)| async move {
            match wait_for_deletion(
                api_ref,
                operation,
                Some(WaitOptions {
                    timeout: Duration::from_secs(120),
                    poll_interval: Duration::from_secs(5),
                }),
            )
            .await
            {
                Ok(_) => {
                    eprintln!("cleanup completed box={sandbox_id}");
                    true
                }
                Err(error) => {
                    eprintln!(
                        "cleanup not confirmed box={sandbox_id} operation={operation}: {error}"
                    );
                    false
                }
            }
        },
    ))
    .await;
    if outcomes.contains(&false) || cleanup_failed {
        return Err(
            "fixture cleanup not confirmed; resource IDs retained in state directory".into(),
        );
    }
    result
}
